use serde::{Deserialize, Serialize};
use byteorder::{BigEndian, ByteOrder};
use std::convert::AsMut;
use std::error::Error;

const MAX_PACKET_SIZE : u64 = 1300;

#[derive(Debug)]
pub enum PacketType {
    Request,
    Response,
    Data,
    Ack,
    Metadata,
    Error,
    None,
}

/// Representation of the Request Packet in memory
#[derive(Clone, Debug)]
pub struct RequestPacket {
    pub connection_id : u32,     // Best way to store 24 Bit ?
    pub fields : u8,
    pub byte_offset : u64,       // 64 Bit
    pub flow_window : u32,
    pub file_name : std::string::String     // This always has to be 255 Bytes ! and can't be done with serde in the way our spec wants :(
}

impl RequestPacket {
    /// Creates a byte representation of a request packet with given parameters in an u8 vector
    pub fn serialize(connection_id : u32, byte_offset : u64, fields : u8, flow_window : u32, file_name : std::string::String) -> Vec<u8>{
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Start capacity needs to be adapted to Request packet's size
        buffer.extend_from_slice(&con_id);
        buffer.push(fields);
        buffer.extend_from_slice(&byte_offset.to_be_bytes());
        buffer.extend_from_slice(&flow_window.to_be_bytes());
        buffer.extend(file_name.bytes());
        return buffer;
    }
    
    /// Parses a slice of u8 received over the network and returns a request packet or Failure
    pub fn deserialize(buffer : &[u8]) -> Result<RequestPacket, &'static str> {
        if buffer.len() > 279 {
            debug!("Could not parse Request. Had invalid length for parsing {:x} expected less than 279.", buffer.len());
            return Err("Parsing Request Packet failed.");
        }
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        let connection_id = u32::from_be_bytes(con_id);
        if connection_id != 0 {
            debug!("Could not parse Request. ConnectionID was {:x} expected to be 0.", connection_id);
            return Err("Parsing Request Packet failed. Invalid ID.");
        }
        Ok(RequestPacket {
            connection_id : connection_id,
            fields : buffer[3],
            byte_offset : BigEndian::read_u64(&buffer[4..12]),
            flow_window : BigEndian::read_u32(&buffer[12..16]),
            file_name : String::from_utf8_lossy(&buffer[16..]).to_string(),     // Might be really expensive
        })
    }
}

/// Representation of the Response Packet in memory
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResponsePacket {
    pub connection_id : u32,     // Best way to store 24 Bit ?
    pub fields : u8,
    pub block_id : u32,       // 32 Bit
    pub file_hash : [u8; 32],        // 256 Bit Hash
    pub file_size : u64
}

impl ResponsePacket {
    /// Creates a byte representation of a response packet with given parameters in an u8 vector
    pub fn serialize(connection_id : u32, block_id : u32, fields : u8, file_hash : [u8; 32], file_size : u64) -> Vec<u8>{
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);
        buffer.extend_from_slice(&con_id);
        buffer.push(fields);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.extend_from_slice(&file_hash);
        buffer.extend(&file_size.to_be_bytes());
        
        return buffer;
    }

    /// Parses a slice of u8 received over the network and returns a response packet or Failure
    pub fn deserialize(buffer : &[u8]) -> Result<ResponsePacket, &'static str> {
        if buffer.len() != 48 {
            debug!("Could not parse Response Packet. Had invalid length for parsing {:x} expected 48.", buffer.len());
            return Err("Parsing ACK Packet");
        }
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        let mut file_hash : [u8; 32] = [0; 32];
        file_hash[..32].copy_from_slice(&buffer[8..41]);    // Real sketchy needs testing
        Ok(ResponsePacket {
            connection_id : u32::from_be_bytes(con_id),
            fields : buffer[3],
            block_id : BigEndian::read_u32(&buffer[4..8]),
            file_hash : file_hash,
            file_size : BigEndian::read_u64(&buffer[41..49]),
        })
    }
}

/// Representation of the Data Packet in memory
#[derive(Clone, Debug)]
pub struct DataPacket {
    pub connection_id : u32,     // Best way to store 24 Bit ?
    pub fields : u8,
    pub block_id : u32,       // 32 Bit
    pub sequence_id : u16,
    pub data : Vec<u8>
}

impl DataPacket {
    /// Creates a byte representation of a data packet with given parameters in an u8 vector
    pub fn serialize(connection_id : u32, block_id : u32, sequence_id : u16, fields: u8, data : Vec<u8>) -> Vec<u8> {
        if data.len() >= 1220{
            println!("Tried to send {} Bytes of data. Can not fit into one package", data.len());
            return Vec::new();
        }
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);
        buffer.extend_from_slice(&con_id);
        buffer.push(fields);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.extend_from_slice(&sequence_id.to_be_bytes());
        buffer.extend(data);
        return buffer;
    }

    /// Parses a slice of u8 received over the network and returns a data packet or Failure
    pub fn deserialize(buffer : &[u8]) -> Result<DataPacket, &'static str> {
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        let seq_id: u16 = BigEndian::read_u16(&buffer[8..10]);
        if seq_id == 0 {
            debug!("Could not parse Data Packet. Had invalid SequenceID {:x}, must not be 0.", seq_id);
            return Err("Parsing Data Packet");
        }
        Ok(DataPacket {
            connection_id : u32::from_be_bytes(con_id),
            fields : buffer[3],
            block_id : BigEndian::read_u32(&buffer[4..8]),
            sequence_id : seq_id,
            data : buffer[10..].to_vec(),
        })
    }
}

/// Representation of the ACK Packet in memory
#[derive(Clone, Debug)]
pub struct AckPacket {
    pub connection_id : u32,     // on the wire just 24 Bits
    pub block_id : u32,       // 32 Bit
    pub fields : u8,
    pub flow_window : u16,
    pub length : u16,
    pub sid_list : Vec<u32>
}


impl AckPacket {
    /// Creates a byte representation of a ACK packet with given parameters in an u8 vector
    pub fn serialize(connection_id : u32, block_id : u32, fields: u8, flow_window : u16, length : u16, sid_list : Vec<u32>) -> Vec<u8> {
        // TODO: Add a check if the sid_list is too long here
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Memory usage might be higher with that capacity
        buffer.extend_from_slice(&con_id);
        buffer.push(fields);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.extend_from_slice(&flow_window.to_be_bytes());
        buffer.extend_from_slice(&length.to_be_bytes());
        for x in sid_list {
            buffer.extend(&x.to_be_bytes());
        }
        return buffer;
    }

    /// Parses a slice of u8 received over the network and returns a ACK packet or Failure
    pub fn deserialize(buffer : &[u8]) -> Result<AckPacket, &'static str> {
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        // Check if list is divisible by 4
        if (buffer.len() - 12) % 4 != 0{
            println!("Could not parse ACK Packet List had invalid length for parsing");
            return Err("Parsing ACK Packet");
        }
        let mut sid_list : Vec<u32> = Vec::new();
        for n in (12..buffer.len()).step_by(4){
            sid_list.push(BigEndian::read_u32(&buffer[n..n+4]));
        }
        Ok (AckPacket {
            connection_id : u32::from_be_bytes(con_id),
            fields : buffer [3],
            block_id : BigEndian::read_u32(&buffer[4..8]),
            flow_window : BigEndian::read_u16(&buffer[8..10]),
            length : BigEndian::read_u16(&buffer[10..12]),
            sid_list : sid_list
        } )
    }
}

/// Representation of the Metadata Packet in memory
#[derive(Clone, Debug)]
pub struct MetadataPacket{
    pub connection_id : u32,     // on the wire just 24 Bits
    pub fields : u8,
    pub block_id : u32,
    pub new_block_size : u16
}

impl MetadataPacket {
    /// Creates a byte representation of a metadata packet with given parameters in an u8 vector
    pub fn serialize(connection_id : u32, block_id : u32, fields: u8, new_block_size : u16) -> Vec<u8> {
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Memory usage might be higher with that capacity
        buffer.extend_from_slice(&con_id);
        buffer.push(fields);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.extend_from_slice(&new_block_size.to_be_bytes());
        return buffer;
    }

    /// Parses a slice of u8 received over the network and returns a Metadata packet or Failure
    pub fn deserialize(buffer : &[u8]) -> Result<MetadataPacket, &'static str> {
        if buffer.len() != 10{
            println!("Size was {}, expected 10.", buffer.len());
            return Err("Malformed metadata packet could not be parsed.");
        }
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        Ok (MetadataPacket {
            connection_id : u32::from_be_bytes(con_id),
            fields : buffer[3],
            block_id : BigEndian::read_u32(&buffer[4..8]),
            new_block_size : BigEndian::read_u16(&buffer[8..10])
        })
    }
}

/// Representation of the Error Packet in memory
#[derive(Clone, Debug)]
pub struct ErrorPacket{
    pub connection_id : u32,     // on the wire just 24 Bits
    pub fields : u8,
    pub block_id : u32,
    pub error_code : u32
}

impl ErrorPacket {
    /// Creates a byte representation of a error packet with given parameters in an u8 vector
    pub fn serialize(connection_id : u32, block_id : u32, fields: u8, error_code : u32) -> Vec<u8> {
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Memory usage might be higher with that capacity
        buffer.extend_from_slice(&con_id);
        buffer.push(fields);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.extend_from_slice(&error_code.to_be_bytes());
        return buffer;
    }

    /// Parses a slice of u8 received over the network and returns a error packet or Failure
    pub fn deserialize(buffer : &[u8]) -> Result<ErrorPacket, &'static str> {
        if buffer.len() != 12{
            println!("Size was {}, expected 12.", buffer.len());
            return Err("Malformed error packet");
        }
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        Ok (ErrorPacket {
            connection_id : u32::from_be_bytes(con_id),
            fields : buffer[3],
            block_id : BigEndian::read_u32(&buffer[4..8]),
            error_code : BigEndian::read_u32(&buffer[8..12])
        })
    }
}

pub fn get_connection_id(packet : &Vec<u8>) -> Result<u32, ()> {
    if packet.len() < 4 {
        warn!("Packet is malformed. Length is too short");
    } else {
        let con_id : [u8; 4] = [0, packet[0], packet[1], packet[2]];
        let connection_id = u32::from_be_bytes(con_id);
        return Ok(connection_id);
    }
    return Err(());
}

pub fn get_packet_type(packet : &Vec<u8>) -> PacketType {
    let con_id : [u8; 4] = [0, packet[0], packet[1], packet[2]];
    let connection_id = u32::from_be_bytes(con_id);
    if connection_id == 0 {
        debug!("Connection ID is 0! Request or response packet!");
        // The packet can only be an request packet
        return PacketType::Request;
    }

    // TODO: Constant offset would only work if all packets had the same structure!
    let flags = packet[7];
    if flags & 0x80 == 0x80 {
        return PacketType::Ack;
    } else if flags & 0x40 == 0x40 {
        return PacketType::Error;
    } else if flags % 0x20 == 0x20 {
        return PacketType::Metadata;
    } else if flags % 0xE0 == 0 {
        return PacketType::Data;
    } else if flags & 0xE0 == 0 {

    }
    // The rest can only be determined in the current state

    PacketType::None
}

pub fn check_packet_type(packet : &Vec<u8>, p_type : PacketType) -> bool {
    let con_id : [u8; 4] = [0, packet[0], packet[1], packet[2]];
    let connection_id = u32::from_be_bytes(con_id);
    match p_type {
        PacketType::Ack => {
            return packet[7] & 0x80 == 0x80;
        },
        PacketType::Data => {
            return packet[7] & 0xE0 == 0x00;
        },
        PacketType::Error => {
            return packet[7] & 0x40 == 0x40;
        },
        PacketType::Metadata => {
            return packet[7] & 0x20 == 0x20;
        }
        PacketType::Request => {
            return connection_id == 0;
        },
        PacketType::Response => {
            return connection_id != 0 && packet[7] & 0xE0 == 0;
        }
        _ => {
            warn!("The packet type cannot be found!");
            return false;
        }
    }
}