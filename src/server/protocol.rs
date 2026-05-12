/// Requests accepted by the network server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    Put { key: Vec<u8>, value: Vec<u8> },
    Get { key: Vec<u8> },
    Delete { key: Vec<u8> },
}

/// Responses returned by the network server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Response {
    Ok,
    Value(Option<Vec<u8>>),
    Error(String),
}

