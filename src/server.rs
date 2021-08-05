
use std::collections::{HashSet, HashMap};
use std::net::{UdpSocket, SocketAddr};
use pretty_hex::*;
use std::io::Read;
use std::fs;
use std::fs::File;
use rand::Rng;
use sha2::{Sha256, Digest};
use std::convert::TryInto;
use std::{thread, time};
use crate::cmdline_handler::Options;
use crate::packets::*;

const PACKET_SIZE : usize = 1280;
const DATA_SIZE : usize = 1270;
const MAX_FLOW_WINDOW : u16 = 100;

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
                        self.send_error(&sock, &addr, ErrorTypes::FileUnavailable);
                        continue;
                    },
                };
                let filesize = file.len() as u64;

                // Compute the checksum
                let mut hasher = Sha256::new();
                hasher.update(&file);
                let hash = hasher.finalize();
                debug!("Generated hash: {}", pretty_hex(&hash));
                let filehash : [u8;32] = match hash.as_slice().try_into() {
                    Ok(h) => h,
                    Err(e) => {
                        warn!("Failed to convert hash: {}", e);
                        self.send_error(&sock, &addr, ErrorTypes::Abort);
                        continue;
                    }
                };

                // Create the new state
                let connection_id = self.generate_conn_id();
                self.create_state(connection_id, filename, filesize, addr);
                
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
                self.sleep_n_ms(150);

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

                    let state = match self.states.get(&ack.connection_id) {
                        Some(s) => s.to_owned(),
                        None => {
                            error!("Connection with ID {} does not exists", ack.connection_id);
                            continue;
                        }
                    };

                    if ack.length > 0 {
                        self.handle_retransmission();
                    } else {
                        // Advance the parameter because of successfull transmission
                        
                        // Cap the maximal flow window
                        let mut flow_window = ack.flow_window;
                        if ack.flow_window > MAX_FLOW_WINDOW {
                            flow_window = MAX_FLOW_WINDOW;
                        }

                        let sent = (DATA_SIZE * state.flow_window as usize) as u64;
                        let new_state = ConnectionStore {
                            state : ConnectionState::Transfer,
                            block_id : state.block_id + 1,
                            flow_window : flow_window,
                            file_size : state.file_size,
                            file : String::from(&state.file),
                            sent : sent,
                            endpoint : state.endpoint,
                        };
                        self.states.insert(connection_id, new_state);
                    }
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
        debug!("Data:\n{}", pretty_hex(&buf));

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
                self.send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                return;
            },
        };


        // TODO: Testing and rework the buffers and size

        // The buffer for the whole next block
        let mut buffer_size = DATA_SIZE * connection.flow_window as usize; 
        // Have to shape this because read_exact does not necessarily read all bytes if the buffer is too large
        let remain = (connection.file_size - connection.sent) as usize;
        let mut iter = connection.flow_window as usize;
        let mut packet_size = DATA_SIZE;
        if buffer_size > remain {
            buffer_size = remain;
            // Computing the iterations (should be auto floored)
            iter = remain / DATA_SIZE;
            if iter < 2 {
                packet_size = remain;
            }
        }
        debug!("Making {} iterations sending {} bytes total in {} byte chunks", iter + 1, buffer_size, packet_size);
        
        let mut block_buffer = vec![0; buffer_size];
        match file.read_exact(&mut block_buffer) {
            Ok(_) => debug!("Read {} bytes from file: {}", buffer_size, filename),
            Err(_) => {
                error!("Failed to read in the next block of file: {}", filename);
                self.send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                return;
            }
        };
        
        // Sending the data packets
        let mut send = 0;

        for i in 0..iter {
            // Creating the current packet
            let mut p_buffer = vec![0; packet_size];
            let start = i as usize * packet_size;
            let mut end = start + packet_size;
            if buffer_size - send < packet_size {
                end = start + (buffer_size - send);
            }
            // TODO: This might be risky. Testing! 
            p_buffer[..].clone_from_slice(&block_buffer[start..end]);

            let data = DataPacket::serialize(connection_id, &connection.block_id, &(i as u16 + 1), &p_buffer);
            let size = match sock.send_to(&data, connection.endpoint) {
                Ok(s) => {
                    debug!("Sent {} bytes to {}", s, connection.endpoint); 
                    s
                },
                Err(e) => {
                    error!("Failed to send data to {}: {}", connection.endpoint, e);
                    self.send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                    self.remove_state(connection_id.to_owned());
                    return;
                }
            };

            // The changes in the connections stats (flow window, sent, etc.) are only made when the ACK arrives
        }

    }

    fn handle_retransmission(&mut self) {

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

    fn create_state(&mut self, connection_id : u32, file_name : &str, size : u64, remote : SocketAddr) {
        self.conn_ids.insert(connection_id);
        let state = ConnectionStore {
            state : ConnectionState::Transfer,
            block_id : 0,
            flow_window : 8,
            file : String::from(file_name),
            file_size : size,
            sent : 0,
            endpoint : remote,
        };
        self.states.insert(connection_id, state);
    }

    fn send_error(&self, sock : &UdpSocket, addr : &SocketAddr, e : ErrorTypes) {
        let val = match e {
            ErrorTypes::FileUnavailable => 0x01,
            ErrorTypes::ConnectionRefused => 0x02,
            ErrorTypes::FileModified => 0x03,
            ErrorTypes::Abort => 0x04,
        };
        let err = ErrorPacket::serialize(&0, &0, &val);
        match sock.send_to(&err, addr) {
            Ok(s) => debug!("Sent {} bytes of error message", s),
            Err(e) => warn!("Failed to send error message: {}", e),
        };
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
