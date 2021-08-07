
use std::{
    io,
    io::Result,
    thread,
    time,
    cmp, 
    fs::File, 
    collections::HashMap,
    net::{UdpSocket, SocketAddr},
    io::{prelude::*, Read, SeekFrom}
};
use pretty_hex::*;
use rand::Rng;
use math::round::ceil;
use crate::cmdline_handler::Options;
use crate::packets::*;
use crate::utils::*;

const MAX_FLOW_WINDOW : u16 = 100;
const DEFAULT_FLOW_WINDOW : u16 = 8;

pub struct TBDServer {
    options : Options,
    states : HashMap<u32, ConnectionStore>,
}

struct ConnectionStore {
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
            states : HashMap::new(),
        }
    }

    pub fn start(&mut self) -> std::io::Result<()> {
        self.server()
    }

    fn server(&mut self) -> std::io::Result<()> {
        
        info!("Starting the server...");
    
        // Bind to given hostname
        let sock = match bind_to_socket(&self.options.hostname, &self.options.server_port, 0) {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        info!("Server is listening on {}", sock.local_addr().unwrap());
    
        loop {
            // 1. Reading a new packet (because this is not threaded block here)

            let (packet, len, addr) = match get_next_packet(&sock, 0.0) {
                Ok(s) => s,
                Err(_) => {
                    error!("Cannot read from socket! Exiting server...");
                    std::process::exit(1);
                }
            };

            // 2. Check for a new client (according to protocol we always get a new request on resumption)
    
            match get_packet_type_server(&packet) {
                PacketType::Request => {

                    let request = match RequestPacket::deserialize(&packet[0..len]) {
                        Ok(p) => p,
                        Err(e) => {
                            error!("Failed to deserialize request packet: {}", e);
                            // Ignore the packet
                            continue;
                        },
                    };

                    // Check for the file status (available, readable)
        
                    let (file, filename) = match get_file(&request.file_name) {
                        Ok(f) => f,
                        Err(_) => {
                            send_error(&sock, &addr, ErrorTypes::FileUnavailable);
                            continue;
                        }
                    };

                    let filesize = file.len() as u64;
                    debug!("File size: {}", filesize);
        
                    // Compute the checksum
                    let hash = match compute_hash(&file) {
                        Ok(h) => h,
                        Err(_) => {
                            send_error(&sock, &addr, ErrorTypes::Abort);
                            continue;
                        }
                    };
        
                    // Limit the flow window to the max of the server
                    let mut flow_window = request.flow_window;
                    if request.flow_window > MAX_FLOW_WINDOW {
                        flow_window = MAX_FLOW_WINDOW;
                    }

                    // Create the new state
                    let connection_id = self.generate_conn_id();
                    self.create_state(connection_id, &filename, filesize, flow_window, request.byte_offset, addr);
                    
                    // TODO: Signal the limit in a metadata packet
        
                    // Construct the answer packet
                    let resp = ResponsePacket::serialize(&connection_id, &0, &hash, &filesize);
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
                    self.sleep_n_ms(100);
        
                    // Start transfer
                    match self.send_next_block(&connection_id, &sock) {
                        _ => continue,
                    }

                },
                // Everything else must be an existing transfer
                PacketType::Error => {
                    let err = match ErrorPacket::deserialize(&packet[0..len]) {
                        Ok(p) => p,
                        Err(e) => {
                            warn!("Failed to deserialize error packet: {}", e);
                            continue;
                        },
                    };
                    warn!("Got an error from {}", addr);
                    warn!("Error Code: {}", err.error_code);
                    info!("Removing connection {}", err.connection_id);
                    self.remove_state(err.connection_id);
                    continue;
                },
                PacketType::Ack => {
                    let ack = match AckPacket::deserialize(&packet[0..len]) {
                        Ok(a) => a,
                        Err(e) => {
                            error!("Failed to deserialize ack packet: {}", e);
                            continue;
                        }
                    };
                    let connection_id = ack.connection_id;
                    
                    // Check if we still got the connection stored
                    match self.states.get(&connection_id) {
                        Some(_) => {/* left empty*/},
                        None => {
                            warn!("Connection with ID {} does not exists", connection_id);
                            continue;
                        }
                    };

                    if ack.length > 0 {
                        // Retransmission
                        
                        self.handle_retransmission(&sock, &ack.sid_list, &connection_id);

                        // We have to do it this way because Rust is shiiit
                        let mut state = match self.states.get_mut(&ack.connection_id) {
                            Some(s) => s,
                            None => {
                                error!("Connection with ID {} does not exists", ack.connection_id);
                                continue;
                            }
                        };

                        // TODO: Update the amount of sent bytes
                        

                        // Update connection parameter
                        state.flow_window = ceil(state.flow_window as f64 / 2.0, 0) as u16;
                        // TODO: Updating flow window and using the smaller window to compute the sent size later
                        
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
                    

                },
                // All other packets should not be received on the server side
                _ => {
                    warn!("Expected a new connection or an acknowledgment but got something else!");
                    // warn!("Expected a new connection or an acknowledgment but got something else! {}", pretty_hex(&packet));
                    continue;
                },
            }
        }   
    }

    fn create_new_sid(&self, file_size : u64, sent : u64, flow_window : u16) -> Vec<u16> {
        let remain = file_size - sent;
        let mut window_size = flow_window as usize * DATA_SIZE;
        if window_size > remain as usize {
            window_size = remain as usize;
        }
        let packets = ceil(window_size as f64 / DATA_SIZE as f64, 0) as u16;
        let mut sid : Vec<u16> = Vec::with_capacity(packets as usize);
        for i in 1..packets + 1 {
            sid.push(i);
        }
        debug!("{:?}", sid);
        sid
    }

    fn send_next_block(&mut self, connection_id : &u32, sock : &UdpSocket) -> io::Result<()> {
        let connection = match self.states.get(connection_id) {
            Some(v) => v,
            None => {
                error!("For the connection ID: {} no connection is found!", connection_id);
                return Err(io::Error::new(io::ErrorKind::InvalidData, "No state found"));
            },
        };

        let sid = self.create_new_sid(connection.file_size, connection.sent, connection.flow_window);
        self.send_block(sock, &sid, connection_id, connection.sent)
    }


    fn handle_retransmission(&mut self, sock : &UdpSocket, sid : &Vec<u16>, connection_id : &u32) -> io::Result<()> {
        let connection = match self.states.get(&connection_id) {
            Some(s) => s,
            None => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "No state found"));
            }
        };
        let offset = connection.sent;
        self.send_block(sock, sid, connection_id, offset)
    }

    fn send_block(&mut self, sock : &UdpSocket, sid : &Vec<u16>, connection_id : &u32, file_offset : u64) -> io::Result<()> {
        let connection = match self.states.get(&connection_id) {
            Some(s) => s,
            None => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "No state found"));
            }
        };

        // Load the buffer at the current offset
        let filename = &connection.file;
        let mut file = match File::open(filename) {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to read in the file {}", filename);
                warn!("{}", e);
                send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                return Err(io::Error::new(io::ErrorKind::NotFound, "File cannot be read"));
            },
        };

        match file.seek(SeekFrom::Start(file_offset)) {
            Ok(o) => o,
            Err(e) => {
                warn!("Failed to read file at offset {}: {}", connection.sent, e);
                send_error(sock, &connection.endpoint, ErrorTypes::FileUnavailable);
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Offset failed"));
            }
        };

        let mut window_size = sid.len() * DATA_SIZE;
        let remain = connection.file_size - connection.sent;
        if window_size > remain as usize {
            window_size = remain as usize;
        }

        let mut window_buffer = vec![0; window_size];
        match file.read_exact(&mut window_buffer) {
            Ok(_) => debug!("Read {} bytes from file: {}", window_size, filename),
            Err(_) => {
                error!("Failed to read in the next block of file: {}", filename);
                send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Failed to read packet in"));
            }
        };

        for seq in sid {

            let mut engine = rand::thread_rng();
            if engine.gen_bool(self.options.p) {
                debug!("Skipping id {}", seq);
                continue;
            }

            // Computing the parameter for the current packet
            let mut size = DATA_SIZE;
            let offset = file_offset + ((seq - 1) as u64 * DATA_SIZE as u64);
            if connection.file_size < file_offset + (*seq as u64 * DATA_SIZE as u64) {
                size = connection.file_size as usize - offset as usize;
            }

            // Reading the data out of the current window
            let (packet, _) = match create_next_packet(&size, &window_buffer, &((*seq - 1) as usize)) {
                Ok(p) => p,
                Err(_) => {
                    send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                    return Err(io::Error::new(io::ErrorKind::WriteZero, "Failed to send next block"));
                }
            };

            let data = DataPacket::serialize(connection_id, &connection.block_id, &(seq), &packet);
            match sock.send_to(&data, connection.endpoint) {
                Ok(s) => {
                    debug!("Sent {} bytes to {}", s, connection.endpoint); 
                    s
                },
                Err(e) => {
                    error!("Failed to send data to {}: {}", connection.endpoint, e);
                    send_error(&sock, &connection.endpoint, ErrorTypes::Abort);
                    self.remove_state(connection_id.to_owned());
                    return Err(io::Error::new(io::ErrorKind::WriteZero, "Failed to send next block"));
                }
            };
        }
        Ok(())
    }

    fn generate_conn_id(&mut self) -> u32 {
        let mut rnd = rand::thread_rng();
        loop {
            let val = rnd.gen_range(1..1 << 23);
            if !self.states.contains_key(&val) {
                return val;
            }
        }
    }

    fn remove_state(&mut self, connection_id : u32) {
        self.states.remove(&connection_id);
    }

    fn create_state(&mut self, connection_id : u32, file_name : &str, size : u64, flow : u16, offset : u64, remote : SocketAddr) {
        let flow = cmp::min(flow, DEFAULT_FLOW_WINDOW);
        let state = ConnectionStore {
            block_id : 0,
            flow_window : flow,
            file : String::from(file_name),
            file_size : size,
            sent : offset,
            endpoint : remote,
        };
        self.states.insert(connection_id, state);
    }
    
    fn sleep_n_ms(&self, ms : u64) {
        let duration = time::Duration::from_millis(ms);
        thread::sleep(duration);
    }
}
