use rand::prelude::*;
use pretty_hex::*;
use std::io::Write;
use std::fs::OpenOptions;
use std::net::{UdpSocket, SocketAddr};
use std::{thread, time};
use std::result::Result;
use std::collections::LinkedList;
use crate::cmdline_handler::Options;
use crate::packets::*;
use crate::net_util::*;

const PACKET_SIZE : u32 = 1280;
const TIMEOUT : u64 = 1;
const START_FLOW_WINDOW : u16 = 8;

pub struct TBDClient {
    options : Options,
    received : u64,
    connection_id : u32,
    flow_window : u16,
    block_id : u32,
    file_hash : [u8; 32],
    file_size : u64,
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
        
        for filename in &self.options.filename {
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
            debug!("Received response: {}", pretty_hex(&packet_buffer));
            
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

            self.receive_data(&sock);            
        }

        info!("Finished file transfer");
    
        Ok(())
    }

    fn bind_to_socket(&self, retries : u32) -> Result<UdpSocket, ()> {
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

    fn receive_next(&self, sock : &UdpSocket, mut buf : &mut [u8]) -> (usize, SocketAddr) {
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

    fn create_ack_list(&self) -> LinkedList<u16> {
        let list = LinkedList::new();
        for i in 0..self.flow_window {
            list.push_back(i);
        }
        list
    }

    fn receive_data(&self, sock : &UdpSocket) {
        // Compute the size of the whole next block
        let buffer_size = PACKET_SIZE * self.flow_window as u32;
        let mut window_buffer = vec![0; buffer_size as usize];
        
        let mut packet_buffer : [u8; PACKET_SIZE as usize] = [0; PACKET_SIZE as usize];
        let mut i = 0;

        loop {
            // TODO: Error handling
            let (len, addr) = self.receive_next(sock, &mut packet_buffer);
            debug!("Received {} bytes from {}:\n{}", len, addr, pretty_hex(&packet_buffer));
            
            if check_packet_type(&packet_buffer.to_vec(), PacketType::Error) {
                let err = match ErrorPacket::deserialize(&packet_buffer[0..len]) {
                    Ok(e) => e,
                    Err(e) => {
                        error!("Failed to deserialize error packet: {}", e);
                    }
                };
                error!("Received an error instead of a data packet: ErrorCode == {}", err.error_code);
                // Abort the file transfer
                std::process::exit(0);
            }

            if !check_packet_type(&packet_buffer.to_vec(), PacketType::Data) {
                error!("Expected data packet but got something else!");
                continue;
            }

            let data = match DataPacket::deserialize(&packet_buffer) {
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
            let start = (data.sequence_id - 1) as usize * PACKET_SIZE as usize;
            // Copy only what we received
            let end = start + len as usize;
            window_buffer[start..end].copy_from_slice(&packet_buffer);

            // TODO: Include the seq id in the ack


            i += 1;
            // break;
        }

        // Writing the current block in correct order into the file
        // self.write_data_to_file(&filename, &window_buffer).unwrap();
    }

    fn handle_retransmission() {
        
    }
    
    fn write_data_to_file(&self, file: &String, data : &Vec<u8>) -> std::io::Result<()> {
        let mut output = match OpenOptions::new().append(true).create(true).open(file) {
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

    fn sleep_n(&self, sec : u64) {
        let duration = time::Duration::from_secs(sec);
        thread::sleep(duration);
    } 
}