use std::{
    cmp,
    fs::OpenOptions,
    io,
    io::Write,
    result::Result,
    collections::LinkedList,
    net::UdpSocket,
};
use rand::Rng;
use pretty_hex::*;
use math::round::ceil;
use crate::cmdline_handler::Options;
use crate::packets::*;
use crate::utils::*;


const TIMEOUT_MS : f64 = 1000.0;
const INIT_TIMEOUT_MS : f64 = TIMEOUT_MS * 3.0;
const START_FLOW_WINDOW : u16 = 8;
const MAX_FLOW_WINDOW : u16 = 40;
const MAX_RETRANSMISSION : usize = 3;

pub struct TBDClient {
    options : Options,
    received : u64,
    offset : u64,
    connection_id : u32,
    flow_window : u16,
    congestion_window : u16,
    block_id : u32,
    file_hash : [u8; 32],
    file_size : u64,
    server : String,
    filename : String,
    retransmission : bool,
    slow_start : bool,
}

impl TBDClient {
    pub fn create(opt : Options) -> TBDClient {
        TBDClient {
            options : opt,
            received : 0,
            offset : 0,
            connection_id : 0,
            flow_window : START_FLOW_WINDOW,
            congestion_window : START_FLOW_WINDOW,
            block_id : 0,
            file_hash : [0; 32],
            file_size : 0,
            server : String::new(),
            filename : String::new(),
            retransmission : false,
            slow_start : true,
        }
    }

    pub fn start(&mut self) -> std::io::Result<()> {
        self.client()
    }

    fn client(&mut self) -> std::io::Result<()> {
        
        info!("Starting the filerequest...");
        
        // Bind to any local IP address (let the system assign one)
        // Try to rebind 3 times, then stop

        let sock = match bind_to_socket(&String::from(&self.options.local_hostname), &self.options.client_port, 3) {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        debug!("Bound to local socket: {:?}", sock.local_addr());

        // Computing the useable name of the server
        self.server = compute_hostname(&self.options.hostname, &self.options.server_port);
        
        // Otherwise we get an error
        let filenames = self.options.filename.clone();
        
        for filename in filenames {
            
            let mut offset = 0;
            if !self.options.overwrite {
                offset = match read_state(&filename) {
                    Ok(o) => {
                        info!("Continue download at offset: {}", o);
                        o
                    },
                    Err(_) => {
                        info!("Starting new download...");
                        0
                    },
                };
            } else {
                info!("Starting new download, overwriting any existing file...");
            }

            // The initial flow window is set by the application implementation
            let request = RequestPacket::serialize(&offset, &self.flow_window, &filename);

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
            let (packet, len, _) = match get_next_packet(&sock, INIT_TIMEOUT_MS) {
                Ok(r) => r,
                Err(_) => return Err(io::Error::new(io::ErrorKind::ConnectionReset, "Did not receive an answer")),
            };
            // debug!("Received response from {}: {}", addr, pretty_hex(&packet_buffer));
            
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
                    self.offset = offset;
                    self.received = offset;
                    self.block_id = res.block_id;
                    self.file_hash = res.file_hash;
                    self.file_size = res.file_size;

                    self.receive_data(&sock);        
                },
                PacketType::Data => {
                    // TODO: Add handling
                    error!("The handling for data packets before the response is currently not implemented!");
                    continue;
                },
                _ => {

                }
            }

            let mut new_file = String::from(filename);
            new_file.push_str(".new");
            let (file, _) = match get_file(&String::from(&new_file)) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let hash = match compute_hash(&file) {
                Ok(h) => h,
                Err(_) => continue,
            };

            if !self.retransmission {
                if !compare_hashes(&self.file_hash.to_vec(), &hash.to_vec()) {
                    error!("The file is corrupted. Hashes do not match!");
                    // TODO: Retransfer the file?
                } else {
                    info!("File hash is correct");
                }
                delete_state(&self.filename);
            } else {
                info!("Did not finish file transfer because retransmission failed.");
            }
        }

        info!("Ending file transfer");
    
        Ok(())
    }

    fn create_new_sid(&mut self, flow_window : u16) -> LinkedList<u16> {
        let mut list = LinkedList::new();
        // Because the ID 0 is blocked for the metadata packet
        for i in 0..flow_window + 1 {
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
        let mut window_size = (DATA_SIZE * self.flow_window as usize) as u64;
        
        // Only read in the min(window,remain)
        window_size = cmp::min(window_size, remain);
        debug!("Remaining: {} Window buffer (before): {} and Window Buffer (after): {}", remain, (DATA_SIZE*self.flow_window as usize), window_size);

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

    fn receive_data(&mut self, sock : &UdpSocket) {
        'outer: loop {
            self.retransmission = false;
            write_state(&self.offset, &self.filename);
            let (iterations, window_size) = self.compute_block_params();
            let sid = self.create_new_sid(iterations);
            let mut window_buffer = vec![0; window_size];

            let mut list = match self.receive(sock, &mut window_buffer, sid) {
                Ok(_) => {
                    if self.received >= self.file_size {
                        info!("Received all packets!");
                        return;
                    }
                    debug!("Finished block {}", self.block_id - 1);
                    continue;
                },
                Err(list) => list,
            };
            self.retransmission = true;
            'inner: for i in 0..MAX_RETRANSMISSION {
                let new_list = match self.receive(sock, &mut window_buffer, list) {
                    Ok(_) => {
                        if self.received >= self.file_size {
                            info!("Received all packets!");
                            self.retransmission = false;
                            return;
                        }
                        debug!("Finished block {}", self.block_id - 1);
                        break 'inner;
                    },
                    Err(list) => {
                        debug!("Failed retransmission {}", i + 1);
                        list
                    },
                };
                list = new_list;
                if i + 1 == MAX_RETRANSMISSION {
                    break 'outer;
                }
            }
        }
    }

    fn receive(&mut self, sock : &UdpSocket, window_buffer : &mut Vec<u8>, list : LinkedList<u16>) -> Result<(), LinkedList<u16>> {
        info!("Starting file transmission...");

        // Prepare working vars
        let mut sid = list.clone();
        let mut i = sid.len();

        debug!("Making {} iterations in the current block {}", i, self.block_id);
        debug!("Expecting: {:?}", list);
        
        let mut loss = false;
        let mut loss_prob;
        let mut engine = rand::thread_rng();

        // Loop until we received i data packets
        'outer: for seq in &list {

            debug!("Waiting for packet: {} with {} left", seq, i);
            
            'inner: loop {
                // Waiting for the next packet
                let (packet, len, _) = match get_next_packet(&sock, TIMEOUT_MS) {
                    Ok(r) => r,
                    Err(None) => {
                        info!("Connection timed out! Starting retransmission...");
                        break 'outer;
                    },
                    Err(_) => { 
                        return Err(sid);
                    },
                };

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
                                break 'inner;
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
    
                        // Loss probability
                        if loss {
                            loss_prob = self.options.q;
                        } else {
                            loss_prob = self.options.p;
                        }

                        if engine.gen_bool(loss_prob) {
                            info!("Skipping packet {}", data.sequence_id);
                            loss = true;
                            continue;
                        }
                        loss = false;

                        let size = self.fill_window_buffer(window_buffer, &data);
                        sid = self.remove_from_list(&sid, data.sequence_id);
    
                        // Advancing the received only after the complete transmission
                        self.received += size;
                        i -= 1;
                        break 'inner;
                    }
                    PacketType::Metadata => {
                        let metadata = match MetadataPacket::deserialize(&packet[0..len]) {
                            Ok(d) => d,
                            Err(e) => {
                                warn!("Failed to deserialize the data packet: {}", e);
                                // This should be fixable in the retransmission (missing seq id)
                                break 'inner;
                            }
                        };
    
                        if metadata.connection_id != self.connection_id {
                            error!("Connection IDs do not match!");
                            // Ignore
                            continue;
                        }
    
                        if metadata.block_id != self.block_id {
                            warn!("Block IDs do not match. Expected {} got {}", self.block_id, metadata.block_id);
                            // Ignore
                            continue;
                        }
                        warn!("New block_size for next round is expected to be {}", metadata.new_block_size);

                        // Updating the congestion window
                        self.congestion_window = metadata.new_block_size;
                        sid = self.remove_from_list(&sid, metadata.sequence_id);

                        break 'inner;
                    }
                    _ => {
                        error!("Expected data packet but got something else!");
                        continue;
                    },
                };
            }
        }

        if sid.len() != 0 {
            info!("Missing {} packets: {:?}", sid.len(), sid);
            // Sending the nack
            let mut vec : Vec<u16> = Vec::with_capacity(sid.len());
            for seq in &sid {
                vec.push(*seq);
            }
            let nack = AckPacket::serialize(&self.connection_id, &self.block_id, &MAX_FLOW_WINDOW, &(sid.len() as u16), &vec);
            match sock.send_to(&nack, &self.server) {
                Ok(_) => {},
                Err(e) => {
                    error!("Failed to send NACK to server: {}", e);
                    error!("Exiting client...");
                    std::process::exit(1);
                }
            }
            return Err(sid);
        } else {
            self.offset += self.flow_window as u64 * DATA_SIZE as u64;
            if self.retransmission {
                // We did a retransmission
                if self.slow_start {
                    self.flow_window = ceil(self.flow_window as f64 / 2.0, 0) as u16;
                    // Slow start has ended
                    self.slow_start = false;
                }else{
                    self.flow_window = ceil(self.congestion_window as f64 / 2.0, 0) as u16;
                }
            } else {
                self.flow_window = self.congestion_window;
            }
            self.block_id += 1;
            
            debug!("Received all packets from the current block {}", self.block_id);
            
            // Sending the acknowledgment
            let sid = Vec::new();
            let ack = AckPacket::serialize(&self.connection_id, &self.block_id, &MAX_FLOW_WINDOW, &0, &sid);
            debug!("Created ACK: {}", pretty_hex(&ack));
            send_data(&ack, &sock, &self.server);
            

            // Writing the current block in correct order into the file
            let filename = String::from(&self.filename);
            if self.block_id > 1 || self.offset > self.flow_window as u64 * DATA_SIZE as u64 {
                self.write_data_to_file(&filename, &window_buffer, false).unwrap();
            } else {
                self.write_data_to_file(&filename, &window_buffer, true).unwrap();
            }

        }
        Ok(())
    }

    fn fill_window_buffer(&mut self, buf : &mut Vec<u8>, data : &DataPacket) -> u64 {
        // Copying the data at the right place into the buffer
        let start = (data.sequence_id - 1) as usize * DATA_SIZE as usize;

        // Compute what is left after the last block received
        let remain = (self.file_size - self.offset) as usize;
        // Compute the size of the current block
        let mut block_size = self.flow_window as usize * DATA_SIZE;
        block_size = cmp::min(block_size, remain);

        if start > block_size {
            error!("The start offset {} is larger than the whole block {}!", start, block_size);
            return 0;
        }

        let end = cmp::min(data.sequence_id as usize * DATA_SIZE, block_size);
        let p_size = end - start;
        // debug!("Remain: {} Block size: {} Start: {} Size: {} End: {}", remain, block_size, start, p_size, end);

        // Safety checks
        if start > buf.len() || end > buf.len() {
            error!("Cannot write from {} to {} into buffer of length {}!", start, end, buf.len());
            return 0;
        }

        if p_size > data.data.len() {
            error!("Cannot read from {} in data with length {}", p_size, data.data.len());
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
}