use serde::{Deserialize, Serialize};
use byteorder::{BigEndian, ByteOrder};

const MAX_PACKET_SIZE : u64 = 1300;

// TODO: In general remove all parameters from the function definitions that are constant anyways (e.g. connection id in a request)
// TODO: Adapt size of the packets in serialize and deserialize. Either make the all fixed length or match the check for the length with the real length. Currently the program is throwing constant errors.

#[derive(Debug, Clone, Eq, PartialEq)]
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
    pub flow_window : u16,
    pub file_name : std::string::String     // This always has to be 255 Bytes ! and can't be done with serde in the way our spec wants :(
}

impl RequestPacket {
    /// Creates a byte representation of a request packet with given parameters in an u8 vector
    pub fn serialize(byte_offset : &u64, flow_window : &u16, file_name : &std::string::String) -> Vec<u8>{
        let con_id = [0, 0, 0];
        let fields : u8 = 0b00000000;
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
            flow_window : BigEndian::read_u16(&buffer[12..14]),
            file_name : String::from_utf8_lossy(&buffer[14..]).to_string(),     // Might be really expensive
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
    pub fn serialize(connection_id : &u32, block_id : &u32, file_hash : &[u8; 32], file_size : &u64) -> Vec<u8>{
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let fields : u8 = 0b10000000;
        let mut buffer : Vec<u8> = Vec::with_capacity(48 as usize);
        buffer.extend_from_slice(&con_id);
        buffer.push(fields);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.extend_from_slice(file_hash);
        buffer.extend(&file_size.to_be_bytes());
        
        return buffer;
    }

    /// Parses a slice of u8 received over the network and returns a response packet or Failure
    pub fn deserialize(buffer : &[u8]) -> Result<ResponsePacket, &'static str> {
        if buffer.len() != 48 {
            debug!("Could not parse Response Packet. Had invalid length for parsing {:x} expected 48.", buffer.len());
            return Err("Parsing Response Packet");
        }
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        let mut file_hash : [u8; 32] = [0; 32];
        file_hash[..32].copy_from_slice(&buffer[8..40]);    // Real sketchy needs testing
        Ok(ResponsePacket {
            connection_id : u32::from_be_bytes(con_id),
            fields : buffer[3],
            block_id : BigEndian::read_u32(&buffer[4..8]),
            file_hash : file_hash,
            file_size : BigEndian::read_u64(&buffer[40..48]),
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
    pub fn serialize(connection_id : &u32, block_id : &u32, sequence_id : &u16, data : &Vec<u8>) -> Vec<u8> {
        if data.len() > 1270 {
            println!("Tried to send {} Bytes of data. Can not fit into one package", data.len());
            return Vec::new();
        }
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);
        let fields : u8 = 0b00000000;
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
        if buffer.len() < 10{
            debug!("Data packet received with data length of 0.");
            return Err("Parsing Data Packet too small");
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
    pub fields : u8,
    pub block_id : u32,       // 32 Bit
    pub flow_window : u16,
    pub length : u16,
    pub sid_list : Vec<u16>
}


impl AckPacket {
    /// Creates a byte representation of a ACK packet with given parameters in an u8 vector
    pub fn serialize(connection_id : &u32, block_id : &u32, flow_window : &u16, length : &u16, sid_list : &Vec<u16>) -> Vec<u8> {
        // TODO: Add a check if the sid_list is too long here
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Memory usage might be higher with that capacity
        let fields : u8 = 0b10000000;   //ACK flag set nothing else
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
        if (buffer.len() - 12) % 2 != 0{
            println!("Could not parse ACK Packet List had invalid length for parsing");
            return Err("Parsing ACK Packet");
        }
        let mut sid_list : Vec<u16> = Vec::new();
        let ack_length : u16 = BigEndian::read_u16(&buffer[10..12]);
        for n in (12..12 + (ack_length as usize) * 2).step_by(2){
            let cur_sid = BigEndian::read_u16(&buffer[n..n+2]);
            if cur_sid == 0{
                return Err("Zero is an invalid SID in ACK Packet.");
            }
            sid_list.push(cur_sid);
        }
        Ok (AckPacket {
            connection_id : u32::from_be_bytes(con_id),
            fields : buffer [3],
            block_id : BigEndian::read_u32(&buffer[4..8]),
            flow_window : BigEndian::read_u16(&buffer[8..10]),
            length : ack_length,
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
    pub sequence_id : u32,
    pub new_block_size : u16
}

impl MetadataPacket {
    /// Creates a byte representation of a metadata packet with given parameters in an u8 vector
    pub fn serialize(connection_id : &u32, block_id : &u32, new_block_size : &u16) -> Vec<u8> {
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Memory usage might be higher with that capacity
        let sequence_id : u32= 0x0;
        let fields : u8 = 0b00000000;
        buffer.extend_from_slice(&con_id);
        buffer.push(fields);
        buffer.extend_from_slice(&block_id.to_be_bytes());
        buffer.extend_from_slice(&sequence_id.to_be_bytes());       // This is the same in every endianness
        buffer.extend_from_slice(&new_block_size.to_be_bytes());
        return buffer;
    }

    /// Parses a slice of u8 received over the network and returns a Metadata packet or Failure
    pub fn deserialize(buffer : &[u8]) -> Result<MetadataPacket, &'static str> {
        if buffer.len() != 14{
            println!("Size was {}, expected 14.", buffer.len());
            return Err("Malformed metadata packet could not be parsed.");
        }
        let con_id : [u8; 4] = [0, buffer[0], buffer[1], buffer[2]];
        let connection_id = u32::from_be_bytes(con_id);
        if connection_id == 0{
            debug!("Received a metadata packet with connection id of 0.");
            return Err("ConnectionID 0 is invalid for Metadata")
        }
        Ok (MetadataPacket {
            connection_id : connection_id,
            fields : buffer[3],
            block_id : BigEndian::read_u32(&buffer[4..8]),
            sequence_id : BigEndian::read_u32(&buffer[8..12]),
            new_block_size : BigEndian::read_u16(&buffer[12..14])
        })
    }
}

pub enum ErrorTypes {
    FileUnavailable,
    ConnectionRefused,
    FileModified,
    Abort,
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
    pub fn serialize(connection_id : &u32, block_id : &u32, error_code : &u32) -> Vec<u8> {
        let con_id_u8s = connection_id.to_be_bytes();
        let con_id = [con_id_u8s[1], con_id_u8s[2], con_id_u8s[3]];
        let mut buffer : Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE as usize);    // Memory usage might be higher with that capacity
        let fields : u8 = 0b01000000;
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
            return packet[3] & 0x80 == 0x80;
        },
        PacketType::Data => {
            return packet[3] & 0xE0 == 0x00;
        },
        PacketType::Error => {
            return packet[3] & 0x40 == 0x40;
        },
        PacketType::Metadata => {
            return packet[3] & 0x20 == 0x20;
        }
        PacketType::Request => {
            return connection_id == 0;
        },
        PacketType::Response => {
            return connection_id != 0 && packet[3] & 0b10000000 == 0b10000000;
        }
        _ => {
            warn!("The packet type cannot be found!");
            return false;
        }
    }
}

pub fn get_packet_type_client(packet : &Vec<u8>) -> PacketType {
    if packet.len() < 10{
        return PacketType::None;
    }
    let fields : u8 = packet[3];
    println!("{:x} {:x}", fields, 0b01000000);
    match fields as u8 {
        0b01000000 => return PacketType::Error,
        0b10000000 => return PacketType::Response,
        // This case distinguishes Data and Metadata
        0b00000000 => {
            let sequence_id : u16 = BigEndian::read_u16(&packet[8..10]);
            if sequence_id == 0{
                return PacketType::Metadata;
            }
            return PacketType::Data;
        },
        _ => {
            debug!("No type matched Fields were {}", fields);
            return PacketType::None;
        }
    }
}

pub fn get_packet_type_server(packet : &Vec<u8>) -> PacketType {
    let con_id = [0, packet[0], packet[1], packet[2]];
    let connection_id = u32::from_be_bytes(con_id);
    if connection_id == 0{
        return PacketType::Request;
    }
    let fields : u8 = packet[3];
    match fields as u8 {
        0b01000000 => return PacketType::Error,
        0b10000000 => return PacketType::Ack,
        _ => {
            debug!("No type matched Fields were {}", fields);
            return PacketType::None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_packet() {
        let base : RequestPacket = RequestPacket{
            connection_id : 0x0,    // Needs to be 0 on connection establishment
            fields : 0x0,
            byte_offset : 0x0,
            flow_window : 0x10,
            file_name : String::from("testfile.txt")
        };
        let ser = RequestPacket::serialize(&base.byte_offset, 
            &base.flow_window, &base.file_name);
        let deser = RequestPacket::deserialize(&ser).unwrap();
        assert_eq!(deser.connection_id, base.connection_id);
        assert_eq!(deser.fields, base.fields);
        assert_eq!(deser.byte_offset, base.byte_offset);
        assert_eq!(deser.flow_window, base.flow_window);
        assert_eq!(deser.file_name, base.file_name);
    }

    #[test]
    fn test_response_packet(){
        let file_hash : [u8;32] = [0x1,0x2,0x3,0x4,0x5,0x6,0x7,0x8,
            0x11,0x12,0x13,0x14,0x15,0x16,0x17,0x18,
            0x1,0x2,0x3,0x4,0x5,0x6,0x7,0x8,
            0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28];
        let base : ResponsePacket = ResponsePacket{
            connection_id : 0x0,    // Needs to be 0 on connection establishment
            fields : 0b10000000,
            block_id : 0x22334455,
            file_hash : file_hash,
            file_size : 0x40000
        };
        let ser = ResponsePacket::serialize(&base.connection_id, 
            &base.block_id, &base.file_hash, &base.file_size);
        let deser = ResponsePacket::deserialize(&ser).unwrap();
        assert_eq!(deser.connection_id, base.connection_id);
        assert_eq!(deser.fields, base.fields);
        assert_eq!(deser.block_id, base.block_id);
        assert_eq!(deser.file_hash, base.file_hash);
        assert_eq!(deser.file_size, base.file_size);
    }

    #[test]
    fn test_data_packet(){
        let mut data : Vec<u8> = Vec::new();
        for i in 0..32 {
            data.push(i);
        }
        let base : DataPacket = DataPacket{
            connection_id : 0x0,    // Needs to be 0 on connection establishment
            fields : 0x0,
            block_id : 0x22334455,
            sequence_id : 0x3355,
            data : data
        };
        let ser = DataPacket::serialize(&base.connection_id, 
            &base.block_id, &base.sequence_id, &base.data);
        let deser = DataPacket::deserialize(&ser).unwrap();
        assert_eq!(deser.connection_id, base.connection_id);
        assert_eq!(deser.fields, base.fields);
        assert_eq!(deser.block_id, base.block_id);
        assert_eq!(deser.sequence_id, base.sequence_id);
        assert_eq!(deser.data, base.data);
    }

    #[test]
    fn test_ack_packet(){
        let mut sid_list : Vec<u16> = Vec::new();
        for i in 0..32 {
            sid_list.push(i);
        }

        let base : AckPacket = AckPacket{
            connection_id : 0x3355,    // Needs to be 0 on connection establishment
            fields : 0b10000000,
            block_id : 0x22334455,
            flow_window : 0x4,
            length : 0x20,
            sid_list : sid_list
        };
        let ser = AckPacket::serialize(&base.connection_id, 
            &base.block_id, &base.flow_window, &base.length, &base.sid_list);
        let deser = AckPacket::deserialize(&ser).unwrap();
        assert_eq!(deser.connection_id, base.connection_id);
        assert_eq!(deser.block_id, base.block_id);
        assert_eq!(deser.fields, base.fields);
        assert_eq!(deser.flow_window, base.flow_window);
        assert_eq!(deser.length, base.length);
        assert_eq!(deser.sid_list, base.sid_list)
    }

    #[test]
    fn test_metadata_packet(){
        let base : MetadataPacket = MetadataPacket{
            connection_id : 0x2244,
            fields : 0b00000000,
            block_id : 0x22334455,
            sequence_id : 0x0,      // Needs to be 0 on connection establishment
            new_block_size : 0x8
        };
        let ser = MetadataPacket::serialize(&base.connection_id, 
            &base.block_id, &base.new_block_size);
        let deser = MetadataPacket::deserialize(&ser).unwrap();
        assert_eq!(deser.connection_id, base.connection_id);
        assert_eq!(deser.fields, base.fields);
        assert_eq!(deser.block_id, base.block_id);
        assert_eq!(deser.sequence_id, base.sequence_id);
        assert_eq!(deser.new_block_size, base.new_block_size);
    }

    #[test]
    fn test_error_packet(){
        let base : ErrorPacket = ErrorPacket{
            connection_id : 0x2244,
            fields : 0b01000000,
            block_id : 0x22334455,
            error_code : 0x12
        };
        let ser = ErrorPacket::serialize(&base.connection_id, 
            &base.block_id, &base.error_code);
        let deser = ErrorPacket::deserialize(&ser).unwrap();
        assert_eq!(deser.connection_id, base.connection_id);
        assert_eq!(deser.fields, base.fields);
        assert_eq!(deser.block_id, base.block_id);
        assert_eq!(deser.error_code, base.error_code);
    }

    #[test]
    fn test_get_packet_client(){
        let base : ErrorPacket = ErrorPacket{
            connection_id : 0x2244,
            fields : 0b01000000,
            block_id : 0x22334455,
            error_code : 0x12
        };
        let ser = ErrorPacket::serialize(&base.connection_id, 
            &base.block_id, &base.error_code);
        let mut check : PacketType = get_packet_type_client(&ser);
        println!("{:?}", check);
        assert_eq!(check, PacketType::Error);
        let base : MetadataPacket = MetadataPacket{
            connection_id : 0x2244,
            fields : 0b00000000,
            block_id : 0x22334455,
            sequence_id : 0x0,      // Needs to be 0 on connection establishment
            new_block_size : 0x8
        };
        let ser = MetadataPacket::serialize(&base.connection_id, 
            &base.block_id, &base.new_block_size);
        check = get_packet_type_client(&ser);
        assert_eq!(check, PacketType::Metadata);
        let mut data : Vec<u8> = Vec::new();
        for i in 0..32 {
            data.push(i);
        }
        let base : DataPacket = DataPacket{
            connection_id : 0x0,    // Needs to be 0 on connection establishment
            fields : 0x0,
            block_id : 0x22334455,
            sequence_id : 0x3355,
            data : data
        };
        let ser = DataPacket::serialize(&base.connection_id, 
            &base.block_id, &base.sequence_id, &base.data);
        check = get_packet_type_client(&ser);
        assert_eq!(check, PacketType::Data);
        let file_hash : [u8;32] = [0x1,0x2,0x3,0x4,0x5,0x6,0x7,0x8,
            0x11,0x12,0x13,0x14,0x15,0x16,0x17,0x18,
            0x1,0x2,0x3,0x4,0x5,0x6,0x7,0x8,
            0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28];
        let base : ResponsePacket = ResponsePacket{
            connection_id : 0x0,    // Needs to be 0 on connection establishment
            fields : 0b10000000,
            block_id : 0x22334455,
            file_hash : file_hash,
            file_size : 0x40000
        };
        let ser = ResponsePacket::serialize(&base.connection_id, 
            &base.block_id, &base.file_hash, &base.file_size);
            check = get_packet_type_client(&ser);
            assert_eq!(check, PacketType::Response);
    }

    #[test]
    fn test_get_packet_server(){
        let base : ErrorPacket = ErrorPacket{
            connection_id : 0x2244,
            fields : 0b01000000,
            block_id : 0x22334455,
            error_code : 0x12
        };
        let ser = ErrorPacket::serialize(&base.connection_id, 
            &base.block_id, &base.error_code);
        let mut check : PacketType = get_packet_type_server(&ser);
        assert_eq!(check, PacketType::Error);
        let base : RequestPacket = RequestPacket{
            connection_id : 0x0,    // Needs to be 0 on connection establishment
            fields : 0x0,
            byte_offset : 0x0,
            flow_window : 0x10,
            file_name : String::from("testfile.txt")
        };
        let ser = RequestPacket::serialize(&base.byte_offset, 
            &base.flow_window, &base.file_name);
        check = get_packet_type_server(&ser);
        assert_eq!(check, PacketType::Request);
        let mut sid_list : Vec<u16> = Vec::new();
        for i in 0..32 {
            sid_list.push(i);
        }
        let base : AckPacket = AckPacket{
            connection_id : 0x3355,    // Needs to be 0 on connection establishment
            fields : 0b10000000,
            block_id : 0x22334455,
            flow_window : 0x4,
            length : 0x20,
            sid_list : sid_list
        };
        let ser = AckPacket::serialize(&base.connection_id, 
            &base.block_id, &base.flow_window, &base.length, &base.sid_list);
        check = get_packet_type_server(&ser);
        assert_eq!(check, PacketType::Ack);
    }
}