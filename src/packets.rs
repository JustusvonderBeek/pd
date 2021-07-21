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
    pub connection_id : [u8; 3],     // Best way to store 24 Bit ?
    pub byte_offset : u64,       // 64 Bit
    pub fields : u8,
    pub flow_window : u32,
    pub file_name : std::string::String     // This always has to be 255 Bytes ! and can't be done with serde in the way our spec wants :(
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResponsePacket {
    pub connection_id : [u8; 3],     // Best way to store 24 Bit ?
    pub block_id : u64,       // 64 Bit
    pub fields : u8,
    pub file_hash : [u8; 32],        // 256 Bit Hash
    pub file_size : u64
}

impl ResponsePacket {
    pub fn serialize(connection_id : u32, block_id : u64, fields : u8, file_hash : [u8; 32], file_size : u64) -> Vec<u8>{
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
    block_id : u64,       // 64 Bit
    sequence_id : u16,
    flags : u8,
    data : Vec<u8>
}

impl DataPacket {
    pub fn serialize(connection_id : u32, block_id : u64, sequence_id : u16, flags: u8, data : Vec<u8>) -> Vec<u8> {
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
        buffer.push(flags);
        buffer.extend(data);
        return buffer;
    }

    pub fn deserialize(buffer : &[u8]) -> DataPacket {
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        DataPacket {
            connection_id : u32::from_be_bytes(con_id),
            block_id : BigEndian::read_u64(&buffer[3..7]),
            sequence_id : BigEndian::read_u16(&buffer[7..9]),
            flags : buffer[9],
            data : buffer[10..].to_vec(),
        }
    }
}