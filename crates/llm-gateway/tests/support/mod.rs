use bm_entry::{
    EntryAcceptedTcpStream, EntryAuthConfig, EntryAuthDecision, EntryBearerPrincipal,
    EntryLocalTransport, EntryOperationCapability,
};
use bm_llm_gateway::GatewayScopeRequest;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};

#[allow(dead_code)]
pub fn gateway_bearer_auth(owner_id: &str) -> EntryAuthDecision {
    let config = EntryAuthConfig::required_bearer_principal(
        "gateway-test-token",
        EntryBearerPrincipal::new(
            "gateway-test-principal",
            owner_id,
            EntryOperationCapability::all().iter().copied(),
        ),
    );
    config.verify_bearer(Some("Bearer gateway-test-token"))
}

#[allow(dead_code)]
pub fn loopback_auth(principal: &str) -> EntryAuthDecision {
    EntryAuthConfig::disabled_for_local()
        .authenticate_local_transport(EntryLocalTransport::InProcess, principal)
}

#[allow(dead_code)]
pub fn loopback_scope_request(principal: &str) -> GatewayScopeRequest {
    GatewayScopeRequest::new(loopback_auth(principal))
}

#[allow(dead_code)]
pub fn accepted_request(
    request: impl Into<Vec<u8>>,
) -> (EntryAcceptedTcpStream, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let addr = listener.local_addr().expect("listener address");
    let request = request.into();
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).expect("loopback client");
        stream.write_all(&request).expect("write gateway request");
        stream
            .shutdown(Shutdown::Write)
            .expect("shutdown gateway request");
        let mut response = Vec::new();
        match stream.read_to_end(&mut response) {
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::UnexpectedEof
                ) => {}
            Err(error) => panic!("read gateway response: {error}"),
        }
        response
    });
    let accepted = EntryAcceptedTcpStream::accept(&listener).expect("accepted loopback peer");
    (accepted, client)
}

#[allow(dead_code)]
pub fn finish_request(client: std::thread::JoinHandle<Vec<u8>>) -> String {
    String::from_utf8(client.join().expect("gateway client")).expect("gateway response UTF-8")
}
