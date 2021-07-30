use serde::{Deserialize, Serialize};
use byteorder::{BigEndian, ByteOrder};

const MAX_PACKET_SIZE : u64 = 1300;

pub struct IntitialRequest {
    pub _type: u8,
    value: u16,
}

impl IntitialRequest {
    
    pub fn create() -> IntitialRequest {
        IntitialRequest {
            _type: 0x12,
            value: 0x1234,
        }
    }
}

pub struct InitialResponse {
    _value : u8,
    _var : u16,
}

impl InitialResponse {

    pub fn create() -> InitialResponse {
        InitialResponse {
            _value : 0x12,
            _var : 0x1234,
        }
    }

}

pub struct Reconnect {

}

pub struct Data {

}

pub struct Confirmation {
    
}

#[derive(Clone, Debug)]
pub struct RequestPacket {
    pub connection_id : u32,     // Best way to store 24 Bit ?
    pub byte_offset : u64,       // 64 Bit
    pub fields : u8,
    pub flow_window : u32,
    pub file_name : std::string::String     // This always has to be 255 Bytes ! and can't be done with serde in the way our spec wants :(
}

impl RequestPacket {
    pub fn serialize(connection_id : u32, byte_offset : u64, fields : u8, flow_window : u32, file_name : std::string::String) -> Vec<u8>{
        let con_id_u8s = connection_id.to_be_bytes();
        let mut con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Start capacity needs to be adapted to Request packet's size
        buffer.extend_from_slice(&con_id);
        buffer.extend_from_slice(&byte_offset.to_be_bytes());
        buffer.push(fields);
        buffer.extend_from_slice(&flow_window.to_be_bytes());
        buffer.extend(file_name.bytes());
        return buffer;
    }
    
    pub fn deserialize(buffer : &[u8]) -> RequestPacket{
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        RequestPacket {
            connection_id : u32::from_be_bytes(con_id),
            byte_offset : BigEndian::read_u64(&buffer[3..7]),
            fields : buffer[7],
            flow_window : BigEndian::read_u32(&buffer[8..12]),
            file_name : String::from_utf8_lossy(&buffer[12..]).to_string(),     // Might be really expensive
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResponsePacket {
    pub connection_id : [u8; 3],     // Best way to store 24 Bit ?
    pub block_id : u32,       // 32 Bit
    pub fields : u8,
    pub file_hash : [u8; 32],        // 256 Bit Hash
    pub file_size : u64
}

impl ResponsePacket {
    pub fn serialize(connection_id : u32, block_id : u32, fields : u8, file_hash : [u8; 32], file_size : u64) -> Vec<u8>{
        let con_id_u8s = connection_id.to_be_bytes();
        // Serialize via serde
       let response = ResponsePacket {
            connection_id : [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]],
            block_id : block_id,
            fields: fields,
            file_hash : file_hash,
            file_size : file_size
        };
        let buffer = bincode::serialize(&response).unwrap();
        return buffer;
    }

    pub fn deserialize(buffer : &[u8]) -> ResponsePacket {
        let des : ResponsePacket = bincode::deserialize(&buffer).unwrap();
        return des;
    }
}

#[derive(Clone, Debug)]
pub struct DataPacket {
    connection_id : u32,     // Best way to store 24 Bit ?
    block_id : u32,       // 32 Bit
    sequence_id : u16,
    fields : u8,
    data : Vec<u8>
}

impl DataPacket {
    pub fn serialize(connection_id : u32, block_id : u32, sequence_id : u16, fields: u8, data : Vec<u8>) -> Vec<u8> {
        if data.len() >= 1220{
            println!("Tried to send {} Bytes of data. Can not fit into one package", data.len());
            return Vec::new();
        }
        let con_id_u8s = connection_id.to_be_bytes();
        let mut con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);
        buffer.extend_from_slice(&con_id);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.extend_from_slice(&sequence_id.to_be_bytes());
        buffer.push(fields);
        buffer.extend(data);
        return buffer;
    }

    pub fn deserialize(buffer : &[u8]) -> DataPacket {
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        DataPacket {
            connection_id : u32::from_be_bytes(con_id),
            block_id : BigEndian::read_u32(&buffer[3..7]),
            sequence_id : BigEndian::read_u16(&buffer[7..9]),
            fields : buffer[9],
            data : buffer[10..].to_vec(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AckPacket {
    connection_id : u32,     // on the wire just 24 Bits
    block_id : u32,       // 32 Bit
    fields : u8,
    flow_window : u16,
    length : u16,
    sid_list : Vec<u32>
}


impl AckPacket {
    pub fn serialize(connection_id : u32, block_id : u32, fields: u8, flow_window : u16, length : u16, sid_list : Vec<u32>) -> Vec<u8> {
        // TODO: Add a check if the sid_list is too long here
        let con_id_u8s = connection_id.to_be_bytes();
        let mut con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Memory usage might be higher with that capacity
        buffer.extend_from_slice(&con_id);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.push(fields);
        buffer.extend_from_slice(&flow_window.to_be_bytes());
        buffer.extend_from_slice(&length.to_be_bytes());
        for x in sid_list {
            buffer.extend(x.to_be_bytes());
        }
        return buffer;
    }

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
            block_id : BigEndian::read_u32(&buffer[3..7]),
            fields : buffer [7],
            flow_window : BigEndian::read_u16(&buffer[8..10]),
            length : BigEndian::read_u16(&buffer[10..12]),
            sid_list : sid_list
        } )
    }
}

/// Representation of the Metadata Packet in memory
#[derive(Clone, Debug)]
pub struct MetadataPacket{
    connection_id : u32,     // on the wire just 24 Bits
    block_id : u32,
    fields : u8,
    new_block_size : u16
}

impl MetadataPacket {
    /// Creates a byte representation of a metadata packet with given parameters in an u8 vector
    pub fn serialize(connection_id : u32, block_id : u32, fields: u8, new_block_size : u16) -> Vec<u8> {
        let con_id_u8s = connection_id.to_be_bytes();
        let mut con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Memory usage might be higher with that capacity
        buffer.extend_from_slice(&con_id);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.push(fields);
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
            block_id : BigEndian::read_u32(&buffer[3..7]),
            fields : buffer[7],
            new_block_size : BigEndian::read_u16(&buffer[8..10])
        })
    }
}

/// Representation of the Error Packet in memory
#[derive(Clone, Debug)]
pub struct ErrorPacket{
    connection_id : u32,     // on the wire just 24 Bits
    block_id : u32,
    fields : u8,
    error_code : u32
}

impl ErrorPacket {
    /// Creates a byte representation of a error packet with given parameters in an u8 vector
    pub fn serialize(connection_id : u32, block_id : u32, fields: u8, error_code : u32) -> Vec<u8> {
        let con_id_u8s = connection_id.to_be_bytes();
        let mut con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Memory usage might be higher with that capacity
        buffer.extend_from_slice(&con_id);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.push(fields);
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
            block_id : BigEndian::read_u32(&buffer[3..7]),
            fields : buffer[7],
            error_code : BigEndian::read_u32(&buffer[8..12])
        })
    }
}