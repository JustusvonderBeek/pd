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