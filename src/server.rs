
use std::collections::{HashSet, HashMap};
use std::net::{UdpSocket, SocketAddr};
use pretty_hex::*;
use std::io::Write;
use std::fs::OpenOptions;
use std::fs;
use rand::Rng;
use sha2::{Sha256, Digest};
use std::convert::TryInto;
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

    pub fn start(&mut self) -> std::io::Result<()> {
        self.server()
    }

    fn server(&mut self) -> std::io::Result<()> {

        info!("Starting the server...");
    
        // Bind to given hostname
        let mut hostname = String::from(&self.options.hostname);
        hostname.push_str(":");
        hostname.push_str(&self.options.local_port.to_string());
        let sock = UdpSocket::bind(hostname).unwrap();
        info!("Server is listening on {}", sock.local_addr().unwrap());
    
        loop {
            // 1. Reading a new packet (because this is not threaded block here)

            let (packet, addr) = self.next_packet(&sock).unwrap();

            // 2. Check for a new client (according to protocol we always get a new request on resumption)
    
            if check_packet_type(&packet, PacketType::Request) {
                // New client
                let packet = match RequestPacket::deserialize(&packet) {
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
                let filename = String::from(packet.file_name);
                let mut file = match fs::read(&filename) {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("Failed to read in the file {}\n{}", filename, e);
                        // TODO: Fail, should we send back an error?
                        continue;
                    },
                };
                // Compute the checksum
                let mut hasher = Sha256::new();
                hasher.update(file);
                let hash = hasher.finalize();
                debug!("Generated hash: {}", pretty_hex(&hash));
                let filehash : [u8;32] = match hash.as_slice().try_into() {
                    Ok(h) => h,
                    Err(e) => {
                        warn!("Failed to convert hash: {}", e);
                        self.remove_state(connection_id);
                        continue;
                    }
                };

                let resp = ResponsePacket::serialize(connection_id, 0, 0, filehash, 0);
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
        } 
        
    }

    fn next_packet(&mut self, sock : &UdpSocket) -> Result<(Vec<u8>, SocketAddr), ()> {
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
