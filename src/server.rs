
use std::collections::{HashSet, HashMap};
use std::net::{UdpSocket, SocketAddr};
use pretty_hex::*;
use std::io::{prelude::*, Read, SeekFrom};
use std::fs;
use std::fs::File;
use rand::Rng;
use sha2::{Sha256, Digest};
use std::convert::TryInto;
use std::{thread, time, cmp};
use math::round::ceil;
use crate::cmdline_handler::Options;
use crate::packets::*;
use crate::net_util::*;

const PACKET_SIZE : usize = 1280;
const DATA_SIZE : usize = 1270;
const MAX_FLOW_WINDOW : u16 = 100;
const DEFAULT_FLOW_WINDOW : u16 = 8;

enum ConnectionState {
    Transfer,
    Retransmission,
}

// TODO: Add the values we need for the operation
pub struct TBDServer {
    options : Options,
    conn_ids : HashSet<u32>,
    states : HashMap<u32, ConnectionStore>,
}

struct ConnectionStore {
    state : ConnectionState,
    block_id : u32,
    flow_window : u16,
    file_size : u64,
    file : String,
    sent : u64,
    endpoint : SocketAddr,
}

impl TBDServer {
    pub fn create(opt: Options) -> TBDServer {
        TBDServer {
            options : opt,
            conn_ids : HashSet::new(),
            states : HashMap::new(),
        }
    }

    pub fn start(&mut self) -> std::io::Result<()> {
        self.server()
    }

    fn server(&mut self) -> std::io::Result<()> {
        
        info!("Starting the server...");
    
        // Bind to given hostname
        let mut hostname = String::from(&self.options.hostname);
        hostname.push_str(":");
        hostname.push_str(&self.options.server_port.to_string());
        let sock = UdpSocket::bind(hostname).unwrap();
        info!("Server is listening on {}", sock.local_addr().unwrap());
    
        loop {
            // 1. Reading a new packet (because this is not threaded block here)

            let (packet, len, addr) = self.get_next_packet(&sock).unwrap();

            // 2. Check for a new client (according to protocol we always get a new request on resumption)
    
            if check_packet_type(&packet, PacketType::Request) {
                // New client
                let packet = match RequestPacket::deserialize(&packet[..len]) {
                    Ok(p) => p,
                    Err(e) => {
                        error!("Failed to deserialize request packet: {}", e);
                        // We cannot do more than continue and wait for the client to send the next packet; basically ignore the packet
                        continue;
                    },
                };

                // Check for the file status (available, readable)

                let filename = String::from(packet.file_name);
                // debug!("Filename: {}", pretty_hex(&filename));

                // Removing whitespace and 0 bytes from the transfer
                let filename = filename.trim().trim_matches(char::from(0));
                debug!("Filename: {}", pretty_hex(&filename));
                let file = match fs::read(&filename) {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("Failed to read in the file {}\n{}", filename, e);
                        send_error(&sock, &addr, ErrorTypes::FileUnavailable);
                        continue;
                    },
                };
                let filesize = file.len() as u64;
                debug!("File size: {}", filesize);

                // Compute the checksum
                let mut hasher = Sha256::new();
                hasher.update(&file);
                let hash = hasher.finalize();
                debug!("Generated hash: {}", pretty_hex(&hash));
                let filehash : [u8;32] = match hash.as_slice().try_into() {
                    Ok(h) => h,
                    Err(e) => {
                        warn!("Failed to convert hash: {}", e);
                        send_error(&sock, &addr, ErrorTypes::Abort);
                        continue;
                    }
                };

                // Create the new state
                let connection_id = self.generate_conn_id();
                self.create_state(connection_id, filename, filesize, packet.flow_window, packet.byte_offset, addr);
                
                // TODO: Case when we disagree with the client flow window?

                // Construct the answer packet
                let resp = ResponsePacket::serialize(&connection_id, &0, &filehash, &filesize);
                match sock.send_to(&resp, addr) {
                    Ok(size) => debug!("Sent {} bytes to {}", size, addr),
                    Err(e) => {
                        warn!("Failed to transfer data to {}: {}", addr, e);
                        self.remove_state(connection_id);
                        // We won't retry sending the response and it does not really make sense to send an error here
                        continue;
                    }
                }

                // Wait a short period of time
                self.sleep_n_ms(500);

                // Start transfer
                self.send_next_block(&connection_id, &sock);

            } else {
                // Existing transfer
                if check_packet_type(&packet, PacketType::Error) {
                    let err = match ErrorPacket::deserialize(&packet) {
                        Ok(p) => p,
                        Err(e) => {
                            warn!("Failed to deserialize error packet: {}", e);
                            continue;
                        },
                    };
                    warn!("Got an error from {}", addr);
                    warn!("Error Code: {}", err.error_code);
                    self.remove_state(err.connection_id);
                    continue;
                }

                let connection_id : u32 = match get_connection_id(&packet) {
                    Ok(id) => id,
                    Err(_) => {
                        warn!("Failed to parse connection ID!");
                        continue;
                    },
                };

                match self.conn_ids.get(&connection_id) {
                    Some(_) => {/* left empty*/},
                    None => {
                        warn!("Connection with ID {} does not exists", connection_id);
                        continue;
                    }
                };

                if check_packet_type(&packet, PacketType::Ack) {
                    let ack = match AckPacket::deserialize(&packet) {
                        Ok(a) => a,
                        Err(e) => {
                            error!("Failed to deserialize ack packet: {}", e);
                            continue;
                        }
                    };

                    if ack.length > 0 {
                        
                        self.handle_retransmission(&sock, &ack.sid_list, &connection_id);

                        // We have to do it this way because Rust is shiiit
                        let mut state = match self.states.get_mut(&ack.connection_id) {
                            Some(s) => s,
                            None => {
                                error!("Connection with ID {} does not exists", ack.connection_id);
                                continue;
                            }
                        };

                        // Update connection parameter
                        state.block_id += 1;
                        state.sent = DATA_SIZE as u64 * state.flow_window as u64;
                        state.flow_window = ceil(state.flow_window as f64 / 2.0, 0) as u16;
                        
                        if state.sent >= state.file_size {
                            info!("File {} successfully transferred! Removing state...", state.file);
                            self.remove_state(connection_id);
                            continue;
                        }
                        
                    } else {

                        let mut state = match self.states.get_mut(&ack.connection_id) {
                            Some(s) => s,
                            None => {
                                error!("Connection with ID {} does not exists", ack.connection_id);
                                continue;
                            }
                        };

                        // Advance the parameter because of successfull transmission
                        debug!("Successfully transmitted block {} of connection {}", state.block_id, connection_id);
                        
                        // Check if the file transfer is complete and the state can be deleted
                        let mut sent = (DATA_SIZE * state.flow_window as usize) as u64; // Over approximation (but if it is too much this should still be fine)
                        if state.sent + sent > state.file_size {
                            info!("File {} successfully transferred! Removing state...", state.file);
                            self.remove_state(connection_id);
                            continue;
                        }
                        sent = state.sent + sent;

                        // Cap the maximal flow window
                        let mut flow_window = ack.flow_window;
                        if ack.flow_window > MAX_FLOW_WINDOW {
                            flow_window = MAX_FLOW_WINDOW;
                        }

                        state.sent = sent;
                        state.block_id += 1;
                        state.flow_window = flow_window;

                        self.send_next_block(&connection_id, &sock);
                    }
                    continue;
                }

                error!("Expected an acknowledgment or error but got something else!\n{}", pretty_hex(&packet));
                // TODO: Should we abort the connection when the packet does not match?
            }
        } 
        
    }

    fn get_next_packet(&mut self, sock : &UdpSocket) -> Result<(Vec<u8>, usize, SocketAddr), ()> {
        let mut buf : [u8; PACKET_SIZE] = [0; PACKET_SIZE];
        debug!("Waiting for new incoming packet");
        let (len, addr) = match sock.recv_from(&mut buf) {
            Ok(l) => l,
            Err(e) => {
                warn!("Failed to receive packet {:?}", e); 
                return Err(());
            }
        };
        debug!("Received {} bytes from {}", len, addr);
        debug!("{}", pretty_hex(&buf));

        Ok((buf.to_vec(), len, addr))
    }

    fn send_next_block(&mut self, connection_id : &u32, sock : &UdpSocket) {
        let connection = match self.states.get(connection_id) {
            Some(v) => v,
            None => {
                error!("For the connection ID: {} no connection is found!", connection_id);
                return;
            },
        };

        let filename = &connection.file;
        let mut file = match File::open(filename) {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to read in the file {}\n{}", filename, e);
                send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                return;
            },
        };

        // Preparing the data for the next block
        let (iterations,window_size) = self.compute_block_params(&connection);

        debug!("Making {} iterations sending {} bytes total in this block", iterations, window_size);
        
        let offset = match file.seek(SeekFrom::Start(connection.sent)) {
            Ok(o) => o,
            Err(e) => {
                warn!("Failed to read file at offset {}: {}", connection.sent, e);
                send_error(sock, &connection.endpoint, ErrorTypes::FileUnavailable);
                return;
            }
        };

        let mut window_buffer = vec![0; window_size];
        match file.read_exact(&mut window_buffer) {
            Ok(_) => debug!("Read {} bytes from file: {}", window_size, filename),
            Err(_) => {
                error!("Failed to read in the next block of file: {}", filename);
                send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                return;
            }
        };
        
        // Sending the data packets
        let mut remain_block = window_size;

        for i in 0..iterations {

            let rand : f64 = 0.35 as f64;
            let mut engine = rand::thread_rng();
            if engine.gen_bool(rand) {
                debug!("Skipping iteration {}", i);
                continue;
            }
            // Creating the current packet
            let mut p_buffer = vec![0; DATA_SIZE];
            let start = i as usize * DATA_SIZE;
            let mut end = start + DATA_SIZE;
            
            // Check if we need to pad the last packet
            if remain_block < DATA_SIZE {
                end = start + remain_block;
                debug!("Pad the last packet with {} bytes", remain_block);
            }

            // Copying the file data in the current packet buffer
            let p_size = end - start;
            p_buffer[0..p_size].clone_from_slice(&window_buffer[start..end]);

            let data = DataPacket::serialize(connection_id, &connection.block_id, &(i + 1), &p_buffer);
            match sock.send_to(&data, connection.endpoint) {
                Ok(s) => {
                    debug!("Sent {} bytes to {}", s, connection.endpoint); 
                    s
                },
                Err(e) => {
                    error!("Failed to send data to {}: {}", connection.endpoint, e);
                    send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                    self.remove_state(connection_id.to_owned());
                    return;
                }
            };

            remain_block -= p_size;

            // The changes in the connections stats (flow window, sent, etc.) are only made when the ACK arrives
        }

    }

    fn compute_block_params(&self, connection : &ConnectionStore) -> (u16, usize) {
        let remain = (connection.file_size - connection.sent) as usize;
        // The buffer size for the whole next block
        let mut window_buffer = DATA_SIZE * connection.flow_window as usize; 
        // Only read in the min(window,remain)
        window_buffer = cmp::min(remain, window_buffer);

        // Compute the iteration counter
        let mut iterations = connection.flow_window; // default

        if window_buffer == remain {
            // In this case it is always valid that we are sending less than a whole block

            // Compute the number of iterations that are executed in this block
            let res = remain as f64 / DATA_SIZE as f64;
            debug!("Iterations: {}", res);
            iterations = ceil(res, 0) as u16;
        }

        (iterations, window_buffer)
    }

    fn handle_retransmission(&mut self, sock : &UdpSocket, sid : &Vec<u16>, connection_id : &u32) {
        let state = match self.states.get(&connection_id) {
            Some(s) => s,
            None => {
                return;
            }
        };

        for seq in sid {
            // Compute the packet size
            let mut p_size = DATA_SIZE;
            let offset = state.sent + ((seq - 1) as u64 * DATA_SIZE as u64);
            if state.file_size < state.sent + (*seq as u64 * DATA_SIZE as u64) {
                p_size = state.file_size as usize - offset as usize;
            }

            let mut p_buffer = vec![0; p_size];

            // Read in the file at the specific offset
            let mut file = match File::open(&state.file) {
                Ok(f) => f,
                Err(e) => {
                    error!("Failed to read in {}!", state.file);
                    return;
                }
            };
            let offset = match file.seek(SeekFrom::Start(offset)) {
                Ok(o) => o,
                Err(e) => {
                    warn!("Failed to read file at offset {}: {}", state.sent, e);
                    send_error(sock, &state.endpoint, ErrorTypes::FileUnavailable);
                    return;
                }
            };

            match file.read_exact(&mut p_buffer) {
                Ok(_) => debug!("Read {} bytes from file: {}", p_size, state.file),
                Err(_) => {
                    error!("Failed to read in the next block of file: {}", state.file);
                    send_error(&sock, &state.endpoint, ErrorTypes::Abort);
                    return;
                }
            };


            let data = DataPacket::serialize(&connection_id, &state.block_id, seq, &p_buffer);
            send_data(&data, &sock, &state.endpoint.to_string());
        }

        // TODO: Maybe find a way to outsmart rust and update the params in here
    }

    fn generate_conn_id(&mut self) -> u32 {
        let mut rnd = rand::thread_rng();
        loop {
            let val = rnd.gen_range(0..2^24-1);
            if !self.conn_ids.contains(&val) {
                return val;
            }
        }
    }

    fn remove_state(&mut self, connection_id : u32) {
        self.conn_ids.remove(&connection_id);
        self.states.remove(&connection_id);
    }

    fn create_state(&mut self, connection_id : u32, file_name : &str, size : u64, flow : u16, offset : u64, remote : SocketAddr) {
        self.conn_ids.insert(connection_id);
        let flow = cmp::min(flow, DEFAULT_FLOW_WINDOW);
        let state = ConnectionStore {
            state : ConnectionState::Transfer,
            block_id : 0,
            flow_window : flow,
            file : String::from(file_name),
            file_size : size,
            sent : offset,
            endpoint : remote,
        };
        self.states.insert(connection_id, state);
    }

    fn update_state(&mut self, connection_id : u32, block_id : u32, flow_window : u16, sent : u64) {
        let mut state = match self.states.get_mut(&connection_id) {
            Some(s) => s,
            None => {
                warn!("For the connection {} no state can be found", connection_id);
                return;
            }
        };
        state.block_id = block_id;
        state.flow_window = flow_window;
        state.sent = sent;
    }   

    fn sleep_n_sec(&self, sec : u64) {
        let duration = time::Duration::from_secs(sec);
        thread::sleep(duration);
    }
    
    fn sleep_n_ms(&self, ms : u64) {
        let duration = time::Duration::from_millis(ms);
        thread::sleep(duration);
    }
}
