
use std::collections::{HashSet, HashMap};
use std::net::{UdpSocket, SocketAddr};
use pretty_hex::*;
use std::io::Write;
use std::fs::OpenOptions;
use std::fs;
use rand::Rng;
use crate::cmdline_handler::Options;
use crate::packets::*;

const DEBUG_PACKET_SIZE : usize = 100;

enum ConnectionState {
    Setup,
    Transfer,
    Retransmission,
    Error,
}

// TODO: Add the values we need for the operation
pub struct TBDServer {
    options : Options,
    conn_ids : HashSet<u32>,
    states : HashMap<u32, ConnectionState>,
}

impl TBDServer {
    pub fn create(opt: Options) -> TBDServer {
        TBDServer {
            options : opt,
            conn_ids : HashSet::new(),
            states : HashMap::new(),
        }
    }

    pub fn start(&self) -> std::io::Result<()> {
        self.server()
    }

    fn server(&self) -> std::io::Result<()> {

        info!("Starting the server...");
    
        // Bind to given hostname
        let mut hostname = String::from(&self.options.hostname);
        hostname.push_str(":");
        hostname.push_str(&self.options.local_port.to_string());
        let sock = UdpSocket::bind(hostname).unwrap();
        info!("Server is listening on {}", sock.local_addr().unwrap());
    
        // Used to keep track of active connections and IDs
        let mut buf : [u8; DEBUG_PACKET_SIZE] = [0; DEBUG_PACKET_SIZE]; // Just 100 B to get a more consice packet overview in the hexdump
        let mut index = 0;
    
        loop {
            // 1. Reading a new packet (because this is not threaded block here)

            let (packet, addr) = self.next_packet(&sock).unwrap();

            // 2. Check for a new client (according to protocol we always get a new request on resumption)
    
            if check_packet_type(&packet, PacketType::Request) {
                // New client
                let packet = match RequestPacket::deserialize(&buf) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("Failed to deserialize request packet: {}", e);
                        // 
                        continue;
                    },
                };

                let connection_id = self.generate_conn_id();
                self.conn_ids.insert(connection_id);
                self.states.insert(connection_id, ConnectionState::Transfer);

                // Generate the response + load the file
                let resp = ResponsePacket::serialize(connection_id, 0, 0, vec![0;32], 0);
                match sock.send_to(&resp, addr) {
                    Ok(size) => debug!("Sent {} bytes to {}", size, addr),
                    Err(_) => {
                        warn!("Failed to transfer data to {}", addr);
                        self.remove_state(connection_id);
                    } 
                }
                continue;
            } else {
                // Existing transfer
                if check_packet_type(&packet, PacketType::Error) {
                    // TODO: Error handling
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

                // handle_retransmission(packet);

                // handle_transmission(packet);
                if check_packet_type(&packet, PacketType::Ack) {
                    let ack = AckPacket::deserialize(&packet).unwrap();
                    // TODO: Check for the state, if the transfer is complete and so on
                    let state = self.states.get(&ack.connection_id);
                    // if state == ConnectionState::
                }

                error!("Expected an acknowledgment or error but got something else!\n{}", pretty_hex(&packet));
            }
    
            // 3. Parsing the packet into a struct
            
            // TODO: Make an universal function for this
            // TODO: Work with this packet

            // if conn_id == 0 { // NEW CLIENT
            //     // In this case the client is new
            //     info!("New client from {} connected", addr);
            //     let r = self.packet_handling(&PacketType::Request, &buf.to_vec());
            //     let data = match r {
            //         Ok(v) => v,
            //         Err(_) => {
            //             warn!("Failed to process client request!");
            //             continue;
            //         }
            //     };

            //     index = (index + 1) % 10;
            //     // Create a new connectionID
            //     let conn_id = 10;
            //     conn_exists.insert(conn_id);
            //     let r = states.insert(conn_id, PacketType::Response);
            //     match r {
            //         None => debug!("Inserted {} with state {:?} into map!", conn_id, PacketType::Response),
            //         Some(_) => debug!("Updated {} to {:?}", conn_id, PacketType::Response)
            //     }

            //     debug!("Returing: {}", pretty_hex(&data));
            //     let r = sock.send_to(&data, addr);
            //     match r {
            //         Ok(size) => debug!("Sent {}", size),
            //         Err(e) => {
            //             warn!("Failed to sent: {}", e); 
            //             continue;
            //         },
            //     }
            //     // 4.1. Handling a new client (Setup)
            // } else { // EXISTING CLIENT
            //     // 2. Handling the packet
            //     // TODO: Check if the given packet is correct
                
            //     let ex = conn_exists.contains(&conn_id);
            //     if !ex {
            //         warn!("The given Connection ID {} does not exist!", conn_id);
            //         continue;
            //     } 

            //     let r = states.get(&conn_id);
            //     let state = match r {
            //         Some(s) => s,
            //         None => { 
            //             warn!("Could not find state for client {}", &conn_id); 
            //             continue; 
            //         },
            //     };
            //     debug!("Found state {:?} for client {}", state, conn_id);

            //     let r = self.packet_handling(state, &buf.to_vec());
            //     let data = match r {
            //         Ok(d) => d,
            //         Err(e) => { 
            //             warn!("Failed to handle packet for client {}", conn_id); 
            //             continue; 
            //         },
            //     };
            //     // let check = packets::check(&buf, PacketType::Request);
                
            //     // 3. Parse the given packet
            //     // TODO: Deserialize method

            //    // 4.2. Handling an old client (Probably Ack or Metadata)

            // }
        } 
        
    }

    fn next_packet(&self, sock : &UdpSocket) -> Result<(Vec<u8>, SocketAddr), ()> {
        let mut buf : [u8; DEBUG_PACKET_SIZE] = [0; DEBUG_PACKET_SIZE];
        debug!("Waiting for new incoming packet");
        let f = sock.recv_from(&mut buf);
        let (len, addr) : (usize, SocketAddr) = match f {
            Ok(l) => l,
            Err(e) => {
                warn!("Failed to {:?}", e); 
                return Err(());
            }
        };
        debug!("Received {} bytes from {}", len, addr);
        debug!("Data:\n{}", pretty_hex(&buf));

        Ok((buf.to_vec(), addr))
    }

    fn generate_conn_id(&self) -> u32 {
        let rnd = rand::thread_rng();
        loop {
            let val = rnd.gen_range(0..2^24-1);
            if !self.conn_ids.contains(&val) {
                return val;
            }
        }
    }

    fn remove_state(&self, connection_id : u32) {
        self.conn_ids.remove(&connection_id);
        self.states.remove(&connection_id);
    }

    fn packet_handling(&self, p_type : &PacketType, packet : &Vec<u8>) -> Result<Vec<u8>, ()> {
        match p_type  {
            PacketType::Request => {
                debug!("Requst packet");
                let req = match RequestPacket::deserialize(&packet) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("Failed to deserialize request packet: {}", e);
                        return Err(());
                    }
                };
                info!("Client requested file: {}", &req.file_name);
                let file = fs::read("Test.txt").unwrap();
    
                debug!("{}", pretty_hex(&file));
                let data = DataPacket::serialize(1234, 0, 0, 0, file);
                return Ok(data);
            },
            PacketType::Ack => {
                // Got an ack from the client
    
            },
            PacketType::Error => {
                // Closing the connection and freeing state
                // conn_exists.remove(1);
            },
            _ => {
                error!("The given packet type {:?} is not expected on the server side!", p_type);
            }
        }
        Err(())
    }
}
