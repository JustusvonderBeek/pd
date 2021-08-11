use std::{
    io,
    thread,
    time,
    cmp, 
    fs::File, 
    collections::HashMap,
    net::{UdpSocket, SocketAddr},
    io::{prelude::*, Read, SeekFrom}
};
use rand::Rng;
use math::round::ceil;
use crate::cmdline_handler::Options;
use crate::packets::*;
use crate::utils::*;

const MAX_FLOW_WINDOW : u16 = 50;
const DEFAULT_FLOW_WINDOW : u16 = 8;
const SLOW_START_THRESH : u16 = u16::MAX;

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
    retransmission : bool,
    slow_start_next_block : u16,
    ssthresh : u16,
    slow_start : bool,
    client_max_flow : u16,
    repair_mode : bool,
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
                            send_error(&sock, &0, &addr, ErrorTypes::FileUnavailable);
                            continue;
                        }
                    };

                    let filesize = file.len() as u64;
                    debug!("File size: {}", filesize);
        
                    // Compute the checksum
                    let hash = match compute_hash(&file) {
                        Ok(h) => h,
                        Err(_) => {
                            send_error(&sock, &0, &addr, ErrorTypes::Abort);
                            continue;
                        }
                    };
        
                    // Limit the flow window to the max of the server
                    let mut flow_window = DEFAULT_FLOW_WINDOW;
                    if flow_window > request.flow_window {
                        flow_window = request.flow_window;
                    }
                    
                    if flow_window > MAX_FLOW_WINDOW {
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
                    self.sleep_n_ms(50);
        
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
                    match self.states.get_mut(&connection_id) {
                        Some(s) => { 
                            s.endpoint = addr;
                        },
                        None => {
                            warn!("Connection with ID {} does not exists", connection_id);
                            continue;
                        }
                    };

                    if ack.length > 0 {
                        // Retransmission
                        let mut connection = match self.states.get_mut(&ack.connection_id) {
                            Some(s) => s,
                            None => {
                                error!("Connection with ID {} does not exists", ack.connection_id);
                                continue;
                            }
                        };
                        let mut new_sid_list;
                        if ack.length == 0xffff{
                            warn!("Entering repair mode with slow start window {} and flow window {}", connection.slow_start_next_block, connection.flow_window);
                            connection.flow_window = ceil((connection.flow_window as f64) / 2.0, 0) as u16;
                            connection.slow_start_next_block = connection.flow_window;
                            let new_block_size = connection.flow_window;
                            // keep in mind that we already halfed our congestion window and don't need to update after this
                            connection.repair_mode = true;
                            new_sid_list = Vec::new();
                            // TODO: Should we resend the metadata packet as well?
                            for i in 1..new_block_size + 1 {
                                new_sid_list.push(i);
                            }
                        }else{
                            new_sid_list = ack.sid_list;
                        }
                        match self.handle_retransmission(&sock, &new_sid_list, &connection_id) {
                            Ok(_) => {},
                            Err(_) => {
                                warn!("Failed the retransmission");
                            },
                        };

                        // WE DO NOT UPDATE THE CONNECTION PARAMETER IN HERE BECAUSE OTHERWISE WE COMPUTE FALSE BLOCK SIZES!

                        // We can only remove the state when we get a positive acknowledgment so were are done here
                        
                    } else {

                        let mut connection = match self.states.get_mut(&ack.connection_id) {
                            Some(s) => s,
                            None => {
                                error!("Connection with ID {} does not exists", ack.connection_id);
                                continue;
                            }
                        };

                        // Advance the parameter because of successful transmission
                        debug!("Successfully transmitted block {} of connection {}", connection.block_id, connection_id);
                        
                        // Check if the file transfer is complete and the state can be deleted
                        // Over approximation (but if it is too much this should still be fine)
                        let mut sent = (DATA_SIZE * connection.flow_window as usize) as u64; 
                        warn!("Setting sent to {} assuming flow window of {}", sent, connection.flow_window);
                        
                        if connection.sent + sent >= connection.file_size {
                            info!("File {} successfully transferred! Removing state...", connection.file);
                            self.remove_state(connection_id);
                            continue;
                        }
                        connection.client_max_flow = ack.flow_window;
                        
                        if connection.retransmission && !connection.repair_mode{

                            // Cut the window in half regardless of the client
                            if connection.slow_start {
                                connection.slow_start = false;
                                connection.slow_start_next_block = ceil(connection.slow_start_next_block as f64 / 2.0, 0) as u16;
                                connection.flow_window = connection.slow_start_next_block;
                                debug!("Set - Max C Flow: {} Slow start: {} SS next block: {} Flow Window: {}", connection.client_max_flow, connection.slow_start, connection.slow_start_next_block, connection.flow_window);
                            } else {
                                // TODO: Original assumption was + 1 from the client perspective
                                connection.slow_start_next_block = ceil(connection.slow_start_next_block as f64 / 2.0, 0) as u16;
                                connection.flow_window = connection.slow_start_next_block;
                                debug!("Set - Max C Flow: {} SS next block: {} Flow Window: {}", connection.client_max_flow, connection.slow_start_next_block, connection.flow_window);
                            }

                            if connection.flow_window > connection.client_max_flow {
                                // Here server never will be too small
                                connection.flow_window = connection.client_max_flow;
                                connection.slow_start_next_block = connection.client_max_flow;
                                connection.slow_start = false;
                                debug!("Set - Max C Flow: {} SS next block: {} Flow Window: {}", connection.client_max_flow, connection.slow_start_next_block, connection.flow_window);
                            }

                            connection.retransmission = false;
                        } else if !connection.repair_mode {

                            // Cap the maximal flow window
                            let mut flow_window;
                            if connection.slow_start {
                                connection.slow_start_next_block = connection.slow_start_next_block * 2;
                                if connection.slow_start_next_block > connection.ssthresh{
                                    // If we exceed the slowstart threshold we reduce our window
                                    connection.slow_start_next_block = connection.ssthresh;
                                    connection.slow_start = false;
                                }
                                flow_window = connection.slow_start_next_block;
                                // debug!("We are in slow start increasing from {} to {}", connection.flow_window, connection.slow_start_next_block);
                                debug!("Set - SS next block: {} Flow Window: {}", connection.slow_start_next_block, flow_window);

                            }else {
                                connection.slow_start_next_block += 1;
                                flow_window = connection.slow_start_next_block;
                                debug!("Set - Normal next block: {} Flow Window: {}", connection.slow_start_next_block, flow_window);

                            }
                            // If we exceed the servers limit we stop increasing
                            if flow_window > MAX_FLOW_WINDOW || flow_window > connection.client_max_flow {
                                debug!("Reached flow window limit");
                                let new_flow = cmp::min(MAX_FLOW_WINDOW, connection.client_max_flow);
                                flow_window = new_flow;
                                connection.slow_start_next_block = new_flow;
                                debug!("Set - Flow next block: {} Flow Window: {}", connection.slow_start_next_block, flow_window);
                            }
                            debug!("Setting flow window to {}", flow_window);
                            connection.flow_window = flow_window;
                        } else {
                            // No resizing required anymore if we went into repair mode
                            error!("resetting repair mode !");
                            connection.repair_mode = false;
                            connection.slow_start = false;
                        }
                        
                        sent = connection.sent + sent;
                        connection.sent = sent;
                        connection.block_id += 1;

                        match self.send_next_block(&connection_id, &sock) {
                            _ => {/* left empty */},
                        }
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
        debug!("SID: {:?}", sid);
        sid
    }

    fn send_next_block(&mut self, connection_id : &u32, sock : &UdpSocket) -> io::Result<()> {
        let mut _sid = vec![0; 1];
        let mut _sent = 0;
        let mut _flow_window = 0;
        {
            let connection = match self.states.get(connection_id) {
                Some(v) => v,
                None => {
                    error!("For the connection ID: {} no connection is found!", connection_id);
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "No state found"));
                },
            };

            info!("Connection flow window is {} that of peer {}", connection.flow_window, connection.client_max_flow);
            let sid_list = self.create_new_sid(connection.file_size, connection.sent, connection.flow_window);
            _sid = sid_list;
            _sent = connection.sent;
            _flow_window = connection.flow_window;

            // After we can successfully satisfy the information request we also need to send a Metadata packet including the size of the next block
            let mut new_block_size;
            // Get how many bytes will be sent after this round
            let mut full_send = ((_sid.len() * DATA_SIZE) as u64) + (connection.sent);
            if full_send >= connection.file_size{
                full_send = connection.file_size;
            }
            let sent_after_this_round = connection.file_size - full_send;
            // Get how many packets are left
            let still_to_send = ceil(sent_after_this_round as f64 / DATA_SIZE as f64, 0) as u16;
            debug!("Sent after {:x} still to do {}", sent_after_this_round, still_to_send);
            if connection.slow_start {
                // We are still in the slow_start phase and our window will double in size
                new_block_size = cmp::min(_flow_window * 2, MAX_FLOW_WINDOW);
                new_block_size = cmp::min(new_block_size, connection.client_max_flow);
            }else{
                // Otherwise we follow an AIMD strategy so the next block in case of no losses is incremented by 1
                new_block_size = cmp::min(_flow_window + 1, MAX_FLOW_WINDOW);
                new_block_size = cmp::min(new_block_size, connection.client_max_flow);
            }
            new_block_size = cmp::min(new_block_size, still_to_send);
            let m_d = MetadataPacket::serialize(connection_id, &connection.block_id, &new_block_size);

            match sock.send_to(&m_d, connection.endpoint) {
                Ok(s) => {
                    debug!("Sent {} bytes of metadata package to {}", s, connection.endpoint); 
                    s
                },
                Err(e) => {
                    error!("Failed to send metadata to {}: {}", connection.endpoint, e);
                    send_error(&sock, connection_id, &connection.endpoint, ErrorTypes::Abort);
                    self.remove_state(connection_id.to_owned());
                    return Err(io::Error::new(io::ErrorKind::WriteZero, "Failed to send next block"));
                }
            };
        }

        self.send_block(sock, &_sid, connection_id, _sent, _flow_window)
    }


    fn handle_retransmission(&mut self, sock : &UdpSocket, sid : &Vec<u16>, connection_id : &u32) -> io::Result<()> {
        let mut _sent = 0;
        let mut _flow_window = 0;
        {
            let connection = match self.states.get_mut(&connection_id) {
                Some(s) => s,
                None => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "No state found"));
                }
            };
            connection.retransmission = true;
            _sent = connection.sent;
            _flow_window = connection.flow_window;
        }
        debug!("Retransmission list: {:?}", sid);
        self.send_block(sock, sid, connection_id, _sent, _flow_window)
    }

    /// Handles the transmission of an entire block including sending the metadata information
    fn send_block(&mut self, sock : &UdpSocket, sid : &Vec<u16>, connection_id : &u32, file_offset : u64, flow_window : u16) -> io::Result<()> {
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
                send_error(&sock, connection_id, &connection.endpoint, ErrorTypes::Abort);
                return Err(io::Error::new(io::ErrorKind::NotFound, "File cannot be read"));
            },
        };

        match file.seek(SeekFrom::Start(file_offset)) {
            Ok(o) => o,
            Err(e) => {
                warn!("Failed to read file at offset {}: {}", connection.sent, e);
                send_error(sock, connection_id, &connection.endpoint, ErrorTypes::FileUnavailable);
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Offset failed"));
            }
        };

        // Attention: We need to read in the whole last block because we "jump around" inside of it to transmit missing packets 
        let mut window_size = flow_window as usize * DATA_SIZE;
        let remain = connection.file_size - connection.sent;
        if window_size > remain as usize {
            window_size = remain as usize;
        }

        let mut window_buffer = vec![0; window_size];
        match file.read_exact(&mut window_buffer) {
            Ok(_) => debug!("Read {} bytes from file: {} at offset: {}", window_size, filename, file_offset),
            Err(_) => {
                error!("Failed to read in the next block of file: {}", filename);
                send_error(&sock, connection_id, &connection.endpoint, ErrorTypes::Abort);
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Failed to read packet in"));
            }
        };

        let mut loss = false;
        let mut loss_prob;
        let mut engine = rand::thread_rng();

        for seq in sid {

            let next_p : Vec<u8>;
            if *seq <= 0 {
                let mut new_block_size;
                if connection.slow_start {
                    // We are still in the slow_start phase and our window will double in size
                    new_block_size = cmp::min(flow_window * 2, MAX_FLOW_WINDOW);
                    new_block_size = cmp::min(new_block_size, connection.client_max_flow);
                } else {
                    // Otherwise we follow an AIMD strategy so the next block in case of no losses is incremented by 1
                    new_block_size = cmp::min(flow_window + 1, MAX_FLOW_WINDOW);
                    new_block_size = cmp::min(new_block_size, connection.client_max_flow);
                }
                let still_to_send = ceil((connection.sent + window_size as u64) as f64 / DATA_SIZE as f64, 0) as u16;
                new_block_size = cmp::min(new_block_size, still_to_send);
                next_p = MetadataPacket::serialize(connection_id, &connection.block_id, &new_block_size);
            } else {
                // Probability for next packet loss
                if loss {
                    loss_prob = self.options.q;
                } else {
                    loss_prob = self.options.p;
                }
    
                if engine.gen_bool(loss_prob) {
                    debug!("Skipping seq id {}", seq);
                    loss = true;
                    continue;
                }
    
                loss = false;
    
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
                        send_error(&sock, connection_id, &connection.endpoint, ErrorTypes::Abort);
                        return Err(io::Error::new(io::ErrorKind::WriteZero, "Failed to send next block"));
                    }
                };
    
                next_p = DataPacket::serialize(connection_id, &connection.block_id, &(seq), &packet);
            }

            match sock.send_to(&next_p, connection.endpoint) {
                Ok(s) => {
                    debug!("Sent {} bytes to {}", s, connection.endpoint); 
                    s
                },
                Err(e) => {
                    error!("Failed to send data to {}: {}", connection.endpoint, e);
                    send_error(&sock, connection_id, &connection.endpoint, ErrorTypes::Abort);
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
            retransmission : false,
            slow_start_next_block : DEFAULT_FLOW_WINDOW,  // Default we start with 8 Blocks according to spec
            ssthresh : SLOW_START_THRESH,
            slow_start : true,
            client_max_flow : u16::MAX,
            repair_mode : false,
        };
        self.states.insert(connection_id, state);
    }
    
    fn sleep_n_ms(&self, ms : u64) {
        let duration = time::Duration::from_millis(ms);
        thread::sleep(duration);
    }
}
