use pretty_hex::*;
use std::io::Write;
use std::fs::OpenOptions;
use std::net::{UdpSocket, SocketAddr};
use std::{thread, time, cmp};
use std::result::Result;
use std::collections::LinkedList;
use math::round::ceil;
use crate::cmdline_handler::Options;
use crate::packets::*;
use crate::net_util::*;

const PACKET_SIZE : usize = 1280;
const DATA_SIZE : usize = 1270;
const TIMEOUT : u64 = 1;
const START_FLOW_WINDOW : u16 = 8;
const DATA_HEADER_SIZE : usize = 10;

pub struct TBDClient {
    options : Options,
    received : u64,
    connection_id : u32,
    flow_window : u16,
    block_id : u32,
    file_hash : [u8; 32],
    file_size : u64,
    server : String,
}

impl TBDClient {
    pub fn create(opt : Options) -> TBDClient {
        TBDClient {
            options : opt,
            received : 0,
            connection_id : 0,
            flow_window : START_FLOW_WINDOW,
            block_id : 0,
            file_hash : [0; 32],
            file_size : 0,
            server : String::new(),
        }
    }

    pub fn start(&mut self) -> std::io::Result<()> {
        self.client()
    }

    fn client(&mut self) -> std::io::Result<()> {
        
        info!("Staring the filerequest...");
        
        // Bind to any local IP address (let the system assign one)
        // Try to rebind 3 times, then stop

        let sock = self.bind_to_socket(3).unwrap();
        debug!("Bound to local socket: {:?}", sock.local_addr());

        // Create request packet for each file in the vector
        // and obtain the file
        let mut servername = String::from(self.options.hostname.trim().trim_matches(char::from(0)));
        servername.push_str(":");
        servername.push_str(&self.options.server_port.to_string());
        self.server = String::from(&servername);
        // Otherwise we get an error
        let filenames = self.options.filename.clone();
        
        for filename in filenames {
            // TODO: Add the handling for the continued file request

            // The initial flow window is set by the application implementation
            let request = RequestPacket::serialize(&0, &self.flow_window, &filename);

            // Sending the request to the server
            match sock.send_to(&request, &servername) {
                Ok(s) => debug!("Send {} bytes to {}", s, servername),
                Err(e) => {
                    error!("Failed to send to {}: {}", servername, e);
                    // In case we fail we abort the sending
                    return Err(e);
                }
            };
            info!("Requested file: {}", filename);

            // Receive response from server
            let mut packet_buffer : [u8; PACKET_SIZE as usize] = [0; PACKET_SIZE as usize];
            let (len, addr) = self.receive_next(&sock, &mut packet_buffer);
            debug!("Received response from {}: {}", addr, pretty_hex(&packet_buffer));
            
            // Check for errors and correct packet
            if check_packet_type(&packet_buffer.to_vec(), PacketType::Error) {
                let err = ErrorPacket::deserialize(&packet_buffer).unwrap();
                warn!("Received and error from the server: ErrorCode == {:x}", err.error_code);
                continue;
            }

            // TODO: Include handle for data packet!

            if !check_packet_type(&packet_buffer.to_vec(), PacketType::Response) {
                error!("Expected a response packet but got something different!");
                continue;
            }

            // Handle the response packet
            let res = match ResponsePacket::deserialize(&packet_buffer[..len]) {
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

            self.receive_data(&sock, filename);        
        }

        info!("Finished file transfer");
    
        Ok(())
    }

    fn bind_to_socket(&mut self, retries : u32) -> Result<UdpSocket, ()> {
        for i in 0..retries {
            let mut addr = String::from("0.0.0.0:");
            let port = self.options.client_port + i;
            addr.push_str(&port.to_string());
            let sock = match std::net::UdpSocket::bind(&addr) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to bind to a local socket {}: {}", addr, e);
                    continue;
                }
            };
            return Ok(sock);
        }
        Err(())
    }

    fn receive_next(&mut self, sock : &UdpSocket, mut buf : &mut [u8]) -> (usize, SocketAddr) {
        loop {
            match sock.recv_from(&mut buf) {
                Ok(r) => break r,
                Err(e) => {
                    warn!("Failed to receive data: {}", e);
                    continue;
                }
            };
        }
    }

    fn create_ack_list(&mut self, packets : u16) -> LinkedList<u16> {
        let mut list = LinkedList::new();
        // Because the ID 0 is blocked for the metadata packet
        for i in 1..packets + 1 {
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

    fn receive_data(&mut self, sock : &UdpSocket, filename : String) {
        info!("Starting file transmission...");

        // Compute the amount of bytes left
        let (iterations, window_size) = self.compute_block_params();

        // Prepare the buffer for the next whole window
        let mut window_buffer = vec![0; window_size]; // Only pure data

        // Prepare working vars
        let mut i = 0;

        debug!("Making {} iterations in the current block {}", iterations, self.block_id);
        let mut list = self.create_ack_list(iterations);
        
        // Loop through all packets of the window
        loop {
            // Creating the storage for the next packet
            let mut packet_buffer = vec![0; PACKET_SIZE];
            
            let (len, addr) = self.receive_next(sock, &mut packet_buffer);
            // debug!("Received {} bytes from {}:\n{}", len, addr, pretty_hex(&packet_buffer));
            debug!("Received {} bytes from {}", len, addr);
            
            if check_packet_type(&packet_buffer.to_vec(), PacketType::Error) {
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

            if !check_packet_type(&packet_buffer.to_vec(), PacketType::Data) {
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
                send_error(&sock, &addr, ErrorTypes::Abort);
                // Ignore
                continue;
            }

            if data.block_id != self.block_id {
                warn!("Block IDs do not match");
                // Ignore
                continue;
            }

            // Copying the data at the right place into the buffer
            // - 1 because the seq id 0 is reserved for metadata but we still want to use the space
            let start = (data.sequence_id - 1) as usize * DATA_SIZE as usize;
            // Copy only what we received (10 bytes header) - expect this to be the last packet
            let p_size = cmp::min(len - DATA_HEADER_SIZE, (self.file_size - self.received) as usize);
            let end = start + p_size;
            debug!("Start: {} P_size: {} End: {}", start, p_size, end);
            window_buffer[start..end].copy_from_slice(&data.data[0..p_size]);

            // Ack handling
            list = self.remove_from_list(&list, data.sequence_id);
            // Advancing the received only after the complete transmission
            self.received += p_size as u64;
            
            i += 1;
            if i == iterations {
                info!("Received all packets for the current block.");
                break;
            }
        }

        if list.len() != 0 {
            info!("Missing {} packets. Starting retransmission.", list.len());
            self.handle_retransmission(&mut window_buffer);
        } else {
            // Things we only do in a successful transmission
            
            debug!("Received all packets from the current block {}", self.block_id);
            
            // Should we increase by one?
            self.flow_window += 1;
            
            // Sending the acknowledgment
            let sid = Vec::new();
            let ack = AckPacket::serialize(&self.connection_id, &self.block_id, &self.flow_window, &0, &sid);
            debug!("Created ACK: {}", pretty_hex(&ack));
            send_data(&ack, &sock, &self.server);
        }

        self.block_id += 1;

        // Writing the current block in correct order into the file
        self.write_data_to_file(&filename, &window_buffer).unwrap();
    }

    fn handle_retransmission(&mut self, window_buffer : &mut Vec<u8>) {
        
    }
    
    fn write_data_to_file(&mut self, file: &String, data : &Vec<u8>) -> std::io::Result<()> {
        let mut file = String::from(file);
        file.push_str(".new");
        let mut output = match OpenOptions::new().truncate(true).write(true).create(true).open(&file) {
            Ok(f) => f,
            Err(e) => {
                error!("Failed to create file {}: {}", file, e);
                return Err(e);
            }
        };
        output.write_all(&data)
    }
    
    fn decode_error(&self, e : ErrorTypes) {
        
    }

    fn sleep_n(&mut self, sec : u64) {
        let duration = time::Duration::from_secs(sec);
        thread::sleep(duration);
    } 
}