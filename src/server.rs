
use std::collections::{HashSet, HashMap};
use std::net::{UdpSocket, SocketAddr};
use pretty_hex::*;
use std::io::Write;
use std::fs::OpenOptions;
use std::fs;
use crate::cmdline_handler::Options;
use crate::packets::*;

const DEBUG_PACKET_SIZE : usize = 100;

// TODO: Add the values we need for the operation
pub struct TBDServer {
    options : Options,
    conn_ids : HashSet<u32>,
}

impl TBDServer {
    pub fn create(opt: Options) -> TBDServer {
        TBDServer {
            options : opt,
            conn_ids : HashSet::new(),
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
        let mut conn_exists = HashSet::new();
        let mut states : HashMap<u32, PacketType> = HashMap::new();

        let mut buf : [u8; DEBUG_PACKET_SIZE] = [0; DEBUG_PACKET_SIZE]; // Just 100 B to get a more consice packet overview in the hexdump
        let mut index = 0;
    
        loop {
            // 1. Reading a new packet (because this is not threaded block here)

            debug!("Waiting for new incoming packet");
            let f = sock.recv_from(&mut buf);
            let (len, addr) : (usize, SocketAddr) = match f {
                Ok(l) => l,
                Err(e) => { warn!("Failed to {:?}", e); continue },
            };
            debug!("Received {} bytes from {}", len, addr);
            debug!("Data:\n{}", pretty_hex(&buf));

            // 2. Extract the connection ID to check for a new connection
    
            let raw_conn_id : [u8; 4] = [0, buf[0], buf[1], buf[2]];
            let conn_id : u32 = u32::from_be_bytes(raw_conn_id);
            debug!("Read connID: {:x}", conn_id);
    
            // 3. Parsing the packet into a struct
            let packet = RequestPacket::deserialize(&buf);
            // TODO: Make an universal function for this
            // TODO: Work with this packet

            if conn_id == 0 { // NEW CLIENT
                // In this case the client is new
                info!("New client from {} connected", addr);
                let r = self.packet_handling(&PacketType::Request, &buf.to_vec());
                let data = match r {
                    Ok(v) => v,
                    Err(_) => {
                        warn!("Failed to process client request!");
                        continue;
                    }
                };

                index = (index + 1) % 10;
                // Create a new connectionID
                let conn_id = 10;
                conn_exists.insert(conn_id);
                let r = states.insert(conn_id, PacketType::Response);
                match r {
                    None => debug!("Inserted {} with state {:?} into map!", conn_id, PacketType::Response),
                    Some(_) => debug!("Updated {} to {:?}", conn_id, PacketType::Response)
                }

                debug!("Returing: {}", pretty_hex(&data));
                let r = sock.send_to(&data, addr);
                match r {
                    Ok(size) => debug!("Sent {}", size),
                    Err(e) => {
                        warn!("Failed to sent: {}", e); 
                        continue;
                    },
                }
                // 4.1. Handling a new client (Setup)
            } else { // EXISTING CLIENT
                // 2. Handling the packet
                // TODO: Check if the given packet is correct
                
                let ex = conn_exists.contains(&conn_id);
                if !ex {
                    warn!("The given Connection ID {} does not exist!", conn_id);
                    continue;
                } 

                let r = states.get(&conn_id);
                let state = match r {
                    Some(s) => s,
                    None => { 
                        warn!("Could not find state for client {}", &conn_id); 
                        continue; 
                    },
                };
                debug!("Found state {:?} for client {}", state, conn_id);

                let r = self.packet_handling(state, &buf.to_vec());
                let data = match r {
                    Ok(d) => d,
                    Err(e) => { 
                        warn!("Failed to handle packet for client {}", conn_id); 
                        continue; 
                    },
                };
                // let check = packets::check(&buf, PacketType::Request);
                
                // 3. Parse the given packet
                // TODO: Deserialize method

               // 4.2. Handling an old client (Probably Ack or Metadata)

            }
        } 
        
    }

    fn packet_handling(&self, p_type : &PacketType, packet : &Vec<u8>) -> Result<Vec<u8>, ()> {
        match p_type  {
            PacketType::Request => {
                debug!("Requst packet");
                let req = RequestPacket::deserialize(&packet);
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
