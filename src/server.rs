
use std::collections::HashSet;
use std::net::{UdpSocket, SocketAddr};
use pretty_hex::*;
use std::io::Write;
use std::fs::OpenOptions;
use std::fs;
use crate::cmdline_handler::Options;
use crate::packets::*;

pub struct TBDServer {
    options : Options,
    // conn_ids : HashSet<u32, _>,

}

impl TBDServer {
    pub fn create(opt: Options) -> TBDServer {
        TBDServer {
            options : opt,
            // conn_ids : HashSet::new(),
        }
    }

    pub fn start(&self) -> std::io::Result<()> {
        self.start_server()
    }

    fn start_server(&self) -> std::io::Result<()> {

        info!("Starting the server...");
    
        let mut hostname = String::from(&self.options.hostname);
        hostname.push_str(":");
        hostname.push_str(&self.options.local_port.to_string());
        let sock = UdpSocket::bind(hostname).unwrap();
        info!("Server is listening on {}", sock.local_addr().unwrap());
    
        // Used to keep track of active connections and IDs
        let mut conn_exists = HashSet::new();
    
        let mut buf : [u8; 100] = [0; 100]; // Just 100 B to get a more consice packet overview in the hexdump
        let mut index = 0;
    
        loop {
            // TODO: Error handling
            // 1. Reading a new packet (because this is not threaded block here)
            debug!("Waiting for new incoming packet");
            let f = sock.recv_from(&mut buf);
            let (addr, len) = match f {
                Ok(l) => l,
                Err(e) => { warn!("Failed to {:?}", e); continue },
            };
            debug!("Received {} bytes from {}", len, addr);
            debug!("Data:\n{}", pretty_hex(&buf));
    
            // 2. Check for a new connection
    
            let raw_conn_id : [u8; 4] = [0, buf[0], buf[1], buf[2]];
            let conn_id : u32 = u32::from_be_bytes(raw_conn_id);
            debug!("Read connID: {:x}", conn_id);
    
            // 3. Parse the given packet
    
            if conn_id == 0 {
                // In this case the client is new
                info!("New client from {} connected", addr);
                self.handling_server_request(PacketType::Request, &buf.to_vec());
    
                index = (index + 1) % 10;
                // Create a new connectionID
                let conn_id = 10;
                conn_exists.insert(conn_id);
    
                // 4.1. Handling a new client (Setup)
            } else {
               // 4.2. Handling an old client (Probably Ack or Metadata)
            }
        } 
        
    }

    fn handling_server_request(&self, p_type : PacketType, packet : &Vec<u8>) -> Result<Vec<u8>, ()> {
        match p_type  {
            PacketType::Request => {
                debug!("Requst packet");
                let req = RequestPacket::deserialize(&packet);
                info!("Client requested {}", &req.file_name);
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
