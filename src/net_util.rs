use std::net::UdpSocket;

pub fn send_data(buf : &Vec<u8>, sock : &UdpSocket, addr : &String) {
    match sock.send_to(&buf, &addr) {
        Ok(s) => debug!("Send {} bytes to {}", s, addr),
        Err(e) => error!("Failed to send to {}: {}", addr, e),
    };
}