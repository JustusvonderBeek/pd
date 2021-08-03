use rand::prelude::*;
use pretty_hex::*;
use std::io::Write;
use std::fs::OpenOptions;
use std::net::UdpSocket;
use std::{thread, time};
use std::result::Result;
use crate::cmdline_handler::Options;
use crate::packets::*;

const DEBUG_PACKET_SIZE : usize = 100;
const PACKET_SIZE : u32 = 1280;
const DEBUG_TIMEOUT_SEC : u64 = 1;

pub struct TBDClient {
    options : Options,
    received : u64,
    connection_id : u32,
    flow_window : u32,
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
            flow_window : 8,
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

            // Initial flow window ? How to set?
            let request = RequestPacket::serialize(0, 0, 0, &self.flow_window, &filename);

            // Sending the request to the server
            
            let size = match sock.send_to(&request, &servername) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to send request to server: {}", e);
                    self.sleep_n(DEBUG_TIMEOUT_SEC);
                    // TODO: Retry the request, change the control flow

                    continue;
                }
            };

            info!("Requested the file: {}", filename);
            debug!("Sent: {} byte", size);

            // Receive response from server
            let mut packet_buffer : [u8; DEBUG_PACKET_SIZE] = [0; DEBUG_PACKET_SIZE];
            let (addr, len) = sock.recv_from(&mut packet_buffer).expect("Failed to receive response from server!");
            debug!("Received response: {}", pretty_hex(&packet_buffer));
            
            // Check for errors and correct packet
            if check_packet_type(&packet_buffer.to_vec(), PacketType::Error) {
                let err = ErrorPacket::deserialize(&packet_buffer).unwrap();
                warn!("Received and error from the server: {}", err.error_code);
                continue;
            }

            if !check_packet_type(&packet_buffer.to_vec(), PacketType::Response) {
                error!("Expected a response packet but got something different!");
                continue;
            }

            // Handle the response packet
            let res = match ResponsePacket::deserialize(&packet_buffer) {
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

            // Create a buffer for the next block used to store incoming packets in the correct order
            let buffer_size = PACKET_SIZE * self.flow_window;
            let mut window_buffer = vec![0; buffer_size as usize];

            let mut i = 0;
            while i < self.flow_window {
                // TODO: Error handling
                let (addr, len) = sock.recv_from(&mut packet_buffer).expect("Failed to receive data from server");
                
                if check_packet_type(&packet_buffer.to_vec(), PacketType::Error) {
                    error!("Received an error instead of a data packet!");
                    break;
                }

                if !check_packet_type(&packet_buffer.to_vec(), PacketType::Data) {
                    error!("Expected data packet but got something else!");
                    continue;
                }

                let data = DataPacket::deserialize(&packet_buffer).unwrap();
                if data.connection_id != self.connection_id {
                    error!("Connection IDs do not match!");
                    continue;
                }

                if data.block_id != self.block_id {
                    warn!("Block IDs do not match");
                    continue;
                }

                // Copying the data at the right place into the buffer
                let start = data.sequence_id as usize * PACKET_SIZE as usize;
                let end = start + PACKET_SIZE as usize;
                window_buffer[start..end].copy_from_slice(&packet_buffer);

                // TODO: Include the seq id in the ack


                i += 1;
            }
            // TODO: Send the ACK


            // Writing the current block in correct order into the file
            self.write_data_to_file(&filename, &window_buffer).unwrap();
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

    fn sleep_n(&self, sec : u64) {
        let duration = time::Duration::from_secs(sec);
        thread::sleep(duration);
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
}