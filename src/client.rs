use rand::prelude::*;
use pretty_hex::*;
use std::io::Write;
use std::fs::OpenOptions;
use crate::cmdline_handler::Options;
use crate::packets::*;

pub struct TBDClient {
    options : Options,

}

impl TBDClient {
    pub fn create(opt : Options) -> TBDClient {
        TBDClient {
            options : opt,
        }
    }

    pub fn start(&self) -> std::io::Result<()> {
        self.start_client()
    }

    fn start_client(&self) -> std::io::Result<()> {
        // 1. Request the file
        
        let sock = std::net::UdpSocket::bind("0.0.0.0:6001").expect("Failed to bind to socket");
        let filename = &self.options.filename[0];
        let request = RequestPacket::serialize(0, 0, 0, 10, filename.to_owned());
        let size = sock.send_to(&request, "127.0.0.1:5001").expect("Failed to send to server");
        debug!("Sent: {} byte", size);
        info!("Requested the file: {}", &self.options.filename[0]);
    
        // 2. Receive response from server
        
        let mut buf : [u8; 1300] = [0; 1300];
        let (addr, len) = sock.recv_from(&mut buf).expect("Failed to receive response from server!");
        debug!("Received response: {}", pretty_hex(&buf));
        // TODO: Setup connection params
    
        // 3. Receive data in the loop
        let (addr, len) = sock.recv_from(&mut buf).expect("Failed to receive data from server");
        // TODO: Change this to more sophisticated mechanism so that we can actually tell what type of packet we got
        self.handling_client_request(PacketType::Data, &buf.to_vec());
    
        // 4. Ack the data and process the metadata
    
        // 5. Close the connection after success
        Ok(())
    }

    fn handling_client_request(&self, p_type : PacketType, packet : &Vec<u8>) {
        match p_type {
            PacketType::Response => {
    
            },
            PacketType::Data => {
                let mut rnd = rand::thread_rng();
                let mut nums : Vec<u8> = (0..100).collect();
                nums.shuffle(&mut rnd);
                debug!("{}", pretty_hex(&nums));
                let mut output = OpenOptions::new().append(true).create(true).open("output.txt").expect("Failed to open file output.txt");
                output.write_all(&nums).expect("Failed to append to file");
            },
            PacketType::Metadata => {
                
            },
            PacketType::Error => {
    
            },
            _ => {
                error!("The given packet type {:?} is not expected on the client side!", p_type);
            }
        }
    }
}