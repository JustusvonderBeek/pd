use std::{
    thread,
    cmp,
    fs,
    fs::OpenOptions,
    time,
    io,
    io::Write,
    time::Duration,
    result::Result,
    collections::LinkedList,
    convert::TryInto,
    net::{UdpSocket, SocketAddr, Ipv4Addr, IpAddr},
};
use pretty_hex::*;
use math::round::ceil;
use sha2::{Sha256, Digest};
use crate::cmdline_handler::Options;
use crate::packets::*;
use crate::utils::*;

const TIMEOUT_MS : f64 = 1000.0;
const START_FLOW_WINDOW : u16 = 8;
const MAX_RETRANSMISSION : usize = 3;

pub struct TBDClient {
    options : Options,
    received : u64,
    offset : u64,
    connection_id : u32,
    flow_window : u16,
    block_id : u32,
    file_hash : [u8; 32],
    file_size : u64,
    server : String,
    filename : String,
}

impl TBDClient {
    pub fn create(opt : Options) -> TBDClient {
        TBDClient {
            options : opt,
            received : 0,
            offset : 0,
            connection_id : 0,
            flow_window : START_FLOW_WINDOW,
            block_id : 0,
            file_hash : [0; 32],
            file_size : 0,
            server : String::new(),
            filename : String::new(),
        }
    }

    pub fn start(&mut self) -> std::io::Result<()> {
        self.client()
    }

    fn client(&mut self) -> std::io::Result<()> {
        
        info!("Staring the filerequest...");
        
        // Bind to any local IP address (let the system assign one)
        // Try to rebind 3 times, then stop

        let sock = match bind_to_socket(&String::from("0.0.0.0"), &self.options.client_port, 3) {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        debug!("Bound to local socket: {:?}", sock.local_addr());

        // Computing the useable name of the server
        self.server = compute_hostname(&self.options.hostname, &self.options.server_port);
        
        // Otherwise we get an error
        let filenames = self.options.filename.clone();
        
        for filename in filenames {
            // TODO: Add the handling for the continued file request

            // The initial flow window is set by the application implementation
            let request = RequestPacket::serialize(&0, &self.flow_window, &filename);

            // Sending the request to the server
            match sock.send_to(&request, &self.server) {
                Ok(s) => debug!("Send {} bytes to {}", s, self.server),
                Err(e) => {
                    error!("Failed to send to {}: {}", self.server, e);
                    // In case we fail we abort the sending because the socket is broken
                    return Err(e);
                }
            };
            info!("Requested file: {}", filename);
            self.filename = String::from(&filename);

            // Receive response from server
            let (packet, len, addr) = match get_next_packet(&sock, 0.0) {
                Ok(r) => r,
                Err(_) => return Err(io::Error::new(io::ErrorKind::WriteZero, "Failed to send next block")),
            };
            // debug!("Received response from {}: {}", addr, pretty_hex(&packet_buffer));
            debug!("Received {} bytes response from {}", len, addr);
            
            // Check for errors and correct packet
            let packet_type = get_packet_type_client(&packet);
            match packet_type {
                PacketType::Error => {
                    let err = match ErrorPacket::deserialize(&packet) {
                        Ok(e) => e,
                        Err(_) => {
                            error!("Failed to deserialize the error packet!");
                            // Because we need to close the connection (do not know what the server wants us to do)
                            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid packet"));
                        }
                    };
                    warn!("Connection {} received error: ErrorCode == {}", err.connection_id, err.error_code);
                    continue;
                },
                PacketType::Response => {
                    let res = match ResponsePacket::deserialize(&packet[0..len]) {
                        Ok(r) => r,
                        Err(e) => {
                            warn!("Failed to deserialize response packet: {}", e);
                            continue;
                        }
                    };

                    // Storing the current file informations
                    self.connection_id = res.connection_id;
                    self.block_id = res.block_id;
                    self.file_hash = res.file_hash;
                    self.file_size = res.file_size;

                    loop {
                        self.receive_next_block(&sock);        
                        if self.received >= self.file_size {
                            break;
                        }
                    }
                },
                PacketType::Data => {
                    // TODO: Add handling
                    error!("The handling for data packets before the response is currently not implemented!");
                    continue;
                },
                _ => {

                }
            }

            let (file, _) = match get_file(&String::from(&filename)) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let hash = match compute_hash(&file) {
                Ok(h) => h,
                Err(_) => continue,
            };

            if compare_hashes(&self.file_hash.to_vec(), &hash.to_vec()) {
                error!("The file is corrupted. Hashes do not match!");
                // TODO: Retransfer the file?
            } else {
                info!("File hash is correct");
            }
            
        }

        info!("Finished file transfer");
    
        Ok(())
    }

    fn create_new_sid(&mut self, flow_window : u16) -> LinkedList<u16> {
        let mut list = LinkedList::new();
        // Because the ID 0 is blocked for the metadata packet
        for i in 1..flow_window + 1 {
            list.push_back(i);
        }
        list
    }

    fn remove_from_list(&mut self, list : &LinkedList<u16>, element : u16) -> LinkedList<u16> {
        let mut new_list : LinkedList<u16> = LinkedList::new();
        for seq in list {
            if seq.to_owned() != element {
                new_list.push_back(seq.to_owned());
            }
        }
        new_list
    }

    fn compute_block_params(&self) -> (u16, usize) {
        let remain = self.file_size - self.received;
        // The buffer size for the whole next block
        let window_size = (DATA_SIZE as u16 * self.flow_window) as u64;
        // Only read in the min(window,remain)
        let window_size = cmp::min(window_size, remain);
        debug!("Computing params with Remaining {} window buffer before {} and min of both {}", remain, (DATA_SIZE*self.flow_window as usize), window_size);

        // Compute the iteration counter
        let mut iterations = self.flow_window; // default

        if window_size == remain {
            // In this case it is always valid that we are sending less than a whole block

            // Compute the number of iterations that are executed in this block
            let res = remain as f64 / DATA_SIZE as f64;
            debug!("Iterations: {}", res);
            iterations = ceil(res, 0) as u16;
        }
        (iterations, window_size as usize)
    }

    fn receive_next_block(&mut self, sock : &UdpSocket) -> io::Result<()> {
        let (iterations, window_size) = self.compute_block_params();
        let sid = self.create_new_sid(iterations);

        self.receive_data(sock, window_size, sid)
    }

    fn receive_data(&mut self, sock : &UdpSocket, window_size : usize, list : LinkedList<u16>) -> io::Result<()> {
        info!("Starting file transmission...");

        // Prepare the buffer for the next whole window
        let mut window_buffer = vec![0; window_size]; // Only pure data

        // Prepare working vars
        let mut sid = list.clone();
        let mut i = sid.len();

        // Stores the offset in the context of the entire file
        let mut block_offset = self.received;

        debug!("Making {} iterations in the current block {}", i, self.block_id);
        
        // Loop until we received i data packets
        loop {
            debug!("Waiting for packet: {}", i);
            // Waiting for the next packet
            let (packet, len, addr) = match get_next_packet(&sock, TIMEOUT_MS) {
                Ok(r) => r,
                Err(_) => {
                    info!("Connection timed out! Starting retransmission...");
                    break;
                }
            };
            debug!("Received {} bytes from {}", len, addr);

            let packet_type = get_packet_type_client(&packet[0..len].to_vec());
            match packet_type {
                PacketType::Error => {
                    let err = match ErrorPacket::deserialize(&packet[0..len]) {
                        Ok(e) => e,
                        Err(e) => {
                            error!("Failed to deserialize error packet: {}", e);
                            // Close the connection. Because we cannot do anything more
                            error!("Exiting client...");
                            std::process::exit(1);
                        }
                    };

                    if err.connection_id == self.connection_id {
                        error!("Received an error instead of a data packet: ErrorCode == {:x}", err.error_code);
                        // Abort the file transfer
                        error!("Exiting client...");
                        std::process::exit(0);
                    } else {
                        warn!("Received an error with wrong connection id");
                        continue;
                    }
                },
                PacketType::Data => {
                    let data = match DataPacket::deserialize(&packet[0..len]) {
                        Ok(d) => d,
                        Err(e) => {
                            warn!("Failed to deserialize the data packet: {}", e);
                            // This should be fixable in the retransmission (missing seq id)
                            continue;
                        }
                    };

                    if data.connection_id != self.connection_id {
                        error!("Connection IDs do not match!");
                        // Ignore
                        continue;
                    }

                    if data.block_id != self.block_id {
                        warn!("Block IDs do not match. Expected {} got {}", self.block_id, data.block_id);
                        // Ignore
                        continue;
                    }

                    let size = self.fill_window_buffer(&mut window_buffer, &data);
                    sid = self.remove_from_list(&sid, data.sequence_id);

                    // Advancing the received only after the complete transmission
                    self.received += size;

                    // TODO: Test
                    i -= 1;
                    if i == 0 {
                        info!("Received all packets for the current block.");
                        break;
                    }
                }
                _ => {
                    error!("Expected data packet but got something else!");
                    continue;
                },
            }
        }

        if sid.len() != 0 {
            info!("Missing {} packets {:?}. Starting retransmission.", sid.len(), sid);
            self.handle_retransmission(&sock, &mut window_buffer, &sid);
        } else {
            // Things we only do in a successful transmission
            
            debug!("Received all packets from the current block {}", self.block_id);
            
            // Should we increase by one?
            self.flow_window += 1;
            // Would only incorrect in the last block so this is irrelevant
            self.offset += self.flow_window as u64 * DATA_SIZE as u64;
            
            // Sending the acknowledgment
            let sid = Vec::new();
            let ack = AckPacket::serialize(&self.connection_id, &self.block_id, &self.flow_window, &0, &sid);
            debug!("Created ACK: {}", pretty_hex(&ack));
            send_data(&ack, &sock, &self.server);
        }

        self.block_id += 1;

        // Writing the current block in correct order into the file
        let filename = String::from(&self.filename);
        if self.block_id > 1 {
            self.write_data_to_file(&filename, &window_buffer, false).unwrap();
        }
        else {
            self.write_data_to_file(&filename, &window_buffer, true).unwrap();
        }

        Ok(())
    }

    fn handle_retransmission(&mut self, sock : &UdpSocket, window_buffer : &mut Vec<u8>, sid : &LinkedList<u16>) -> io::Result<()> {
        // Waiting for the data
        let mut l_sid : LinkedList<u16> = sid.clone();
        for i in 0..MAX_RETRANSMISSION {

            // Sending the NACK
            let mut sid_vec : Vec<u16> = Vec::with_capacity(l_sid.len());
            for i in &l_sid {
                sid_vec.push(*i);
            }

            let nack = AckPacket::serialize(&self.connection_id, &self.block_id, &self.flow_window, &(sid.len() as u16), &sid_vec);
            match sock.send_to(&nack, &self.server) {
                Ok(_) => {},
                Err(e) => {
                    error!("Failed to send NACK packet: {}", e);
                    error!("Exiting file transfer");
                    std::process::exit(1);
                }
            }

            for _ in 0..l_sid.len() {
                let mut packet_buffer = vec![0; PACKET_SIZE];
                let mut len : usize = 0;
                let mut addr : SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
                
                // TODO: Timeout
                let (w, x, y) = match get_next_packet(&sock, TIMEOUT_MS) {
                    Ok(s) => s,
                    Err(Some(_)) => {
                        info!("Connection timed out! Starting retransmission...");
                        break;
                    },
                    Err(_) => {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "Failed receiving"));
                    }
                };
                len = x;
                addr = y;

                debug!("Received {} bytes from {}", len, addr);
                
                if get_packet_type_client(&packet_buffer.to_vec()) == PacketType::Error {
                    let err = match ErrorPacket::deserialize(&packet_buffer[0..len]) {
                        Ok(e) => e,
                        Err(e) => {
                            error!("Failed to deserialize error packet: {}", e);
                            // Close the connection. Because we cannot do anything more
                            std::process::exit(1);
                        }
                    };
                    if err.connection_id == self.connection_id {
                        error!("Received an error instead of a data packet: ErrorCode == {}", err.error_code);
                        // Abort the file transfer
                        std::process::exit(0);
                    } else {
                        warn!("Received an error with wrong connection id");
                        continue;
                    }
                }
    
                if get_packet_type_client(&packet_buffer.to_vec()) != PacketType::Data {
                    error!("Expected data packet but got something else!");
                    continue;
                }
    
                let data = match DataPacket::deserialize(&packet_buffer[0..len]) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!("Failed to deserialize the data packet: {}", e);
                        // This should be fixable in the retransmission (missing seq id)
                        continue;
                    }
                };
    
                if data.connection_id != self.connection_id {
                    error!("Connection IDs do not match!");
                    send_error(&sock, &self.connection_id, &addr, ErrorTypes::Abort);
                    // Ignore
                    continue;
                }
    
                if data.block_id != self.block_id {
                    warn!("Block IDs do not match. Expected {} got {}", self.block_id, data.block_id);
                    // Ignore
                    continue;
                }
    
                let start = (data.sequence_id - 1) as usize * DATA_SIZE as usize;
                // Copy only what we received (10 bytes header) - expect this to be the last packet
                let p_size = cmp::min(len - DATA_HEADER, (self.file_size - self.received) as usize);
                let end = start + p_size;
                debug!("Start: {} P_size: {} End: {}", start, p_size, end);
                window_buffer[start..end].copy_from_slice(&data.data[0..p_size]);
                
                l_sid = self.remove_from_list(&l_sid, data.sequence_id);
                
                if l_sid.len() == 0 {
                    debug!("Received all packets!");
                    break;
                }

            }

            if l_sid.len() == 0 {
                // Finished transfer
                break;
            }

            if i + 1 == MAX_RETRANSMISSION {
                error!("Failed to receive all packets in the retransmission! Terminating...");
                std::process::exit(1);
            }

        }

        // Sending the acknowledgment
        let sid_vec = Vec::new();
        self.flow_window = ceil(self.flow_window as f64 / 2.0, 0) as u16;
        let ack = AckPacket::serialize(&self.connection_id, &self.block_id, &self.flow_window, &0, &sid_vec);
        debug!("Created ACK: {}", pretty_hex(&ack));
        send_data(&ack, &sock, &self.server);

        // Writing the buffers is done by the calling function
        Ok(())

    }

    fn fill_window_buffer(&mut self, buf : &mut Vec<u8>, data : &DataPacket) -> u64 {
        // TODO: Implement and test

        // Copying the data at the right place into the buffer
        let start = (data.sequence_id - 1) as usize * DATA_SIZE as usize;

        // Compute what is left after the last block received
        let mut remain = (self.file_size - self.offset) as usize;
        // Compute the size of the current block
        let mut block_size = self.flow_window as usize * DATA_SIZE;
        if block_size > remain {
            // This means we need to cap the last packet!
            block_size = remain;
        }

        if start > block_size {
            error!("The start offset {} is larger than the whole block {}!", start, block_size);
            return 0;
        }

        let end = cmp::min(data.sequence_id as usize * DATA_SIZE, block_size - start);
        let p_size = end - start;
        debug!("Remain: {} Block size: {} Start {} Size: {} End: {}", remain, block_size, start, p_size, end);

        // Safety checks
        if start > buf.len() || end > buf.len() {
            error!("Cannot write from {} to {} into buffer of length {}!", start, end, buf.len());
            return 0;
        }

        if start > data.data.len() || end > data.data.len() {
            error!("Cannot read from {} to {} in data with length {}", start, end,data.data.len());
            return 0;
        }

        // Copying the data
        buf[start..end].copy_from_slice(&data.data[0..p_size]);

        p_size as u64
    }

    fn write_data_to_file(&mut self, file: &String, data : &Vec<u8>, trunc : bool) -> std::io::Result<()> {
        let mut file = String::from(file);
        file.push_str(".new");
        
        if trunc {
            let mut output = match OpenOptions::new().write(true).truncate(true).create(true).open(&file) {
                Ok(f) => f,
                Err(e) => {
                    error!("Failed to create file {}: {}", file, e);
                    return Err(e);
                }
            };
            return output.write_all(&data);
        } else {
            let mut output = match OpenOptions::new().write(true).append(true).create(true).open(&file) {
                Ok(f) => f,
                Err(e) => {
                    error!("Failed to create file {}: {}", file, e);
                    return Err(e);
                }
            };
            return output.write_all(&data);
        }
    }

    fn sleep_n_ms(&mut self, ms : u64) {
        let duration = time::Duration::from_millis(ms);
        thread::sleep(duration);
    } 
}