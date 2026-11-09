//! Classification of failed rail RPC calls. A plugin reports a call that got no answer (a timeout
//! or a connection, DNS or TLS failure) with [`NETWORK_UNAVAILABLE`]. Callers read the class with
//! [`classify`] and never parse messages.
use crate::{error::Error, transport::rpc};

/// The error code (and CLI exit code) of a call that got no answer. Nothing was observed, so a
/// retry with the same request ID is safe.
pub const NETWORK_UNAVAILABLE: i32 = 7;
/// The stable name of [`NETWORK_UNAVAILABLE`] in JSON errors and engine replies.
pub const NETWORK_UNAVAILABLE_NAME: &str = "network_unavailable";
/// The error code of an operation whose outcome is unknown ([`Error::uncertain`]).
pub const UNKNOWN: i32 = 5;

/// The class of a failed call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The request got no answer or the connection failed. Retry with the same request ID.
    NetworkUnavailable,
    /// An answer arrived, but the outcome of the operation is unknown.
    Unknown,
    /// Any other failure.
    Other,
}

/// The class of a client error.
pub fn classify(error: &Error) -> Failure {
    match error.code {
        NETWORK_UNAVAILABLE => Failure::NetworkUnavailable,
        UNKNOWN => Failure::Unknown,
        _ => Failure::Other,
    }
}

/// The class of a bounded RPC transport error.
pub fn classify_rpc(error: &rpc::Error) -> Failure {
    match error.kind {
        rpc::ErrorKind::Timeout | rpc::ErrorKind::Connection => Failure::NetworkUnavailable,
        rpc::ErrorKind::Transport | rpc::ErrorKind::Read => Failure::Unknown,
        _ => Failure::Other,
    }
}

/// A network-unavailable error with a retry hint.
pub fn network_unavailable(context: impl std::fmt::Display) -> Error {
    let mut error = Error::uncertain(format!(
        "{context}: network unavailable; retry with the same request ID"
    ));
    error.code = NETWORK_UNAVAILABLE;
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::rpc::{EnvelopeValidation, RpcClient};
    use serde_json::json;
    use std::{net::TcpListener, thread, time::Duration};

    fn call(url: &str, timeout: Duration) -> rpc::Error {
        RpcClient::new(url, timeout, 1 << 16)
            .unwrap()
            .call_with_id(
                "ping",
                &json!({}),
                1,
                EnvelopeValidation::StrictIdentity,
                None,
            )
            .unwrap_err()
    }

    #[test]
    fn a_call_without_an_answer_is_network_unavailable() {
        // A fake transport that accepts the request and never answers: a timeout.
        let silent = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", silent.local_addr().unwrap());
        let held = thread::spawn(move || {
            let (stream, _) = silent.accept().unwrap();
            thread::sleep(Duration::from_millis(1500));
            drop(stream);
        });
        let error = call(&url, Duration::from_millis(300));
        held.join().unwrap();
        assert_eq!(classify_rpc(&error), Failure::NetworkUnavailable);
        // Nothing listens: the connection fails.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        assert_eq!(
            classify_rpc(&call(&url, Duration::from_secs(5))),
            Failure::NetworkUnavailable
        );
    }

    #[test]
    fn client_errors_keep_their_class() {
        let error = network_unavailable("rail RPC");
        assert_eq!(classify(&error), Failure::NetworkUnavailable);
        assert_eq!(
            error.message,
            "rail RPC: network unavailable; retry with the same request ID"
        );
        assert_eq!(classify(&Error::uncertain("x")), Failure::Unknown);
        assert_eq!(classify(&Error::config("x")), Failure::Other);
        assert_eq!(classify(&Error::api(409, "x")), Failure::Other);
    }

    /// Every registered plugin reports a call that got no answer with the shared code.
    #[test]
    fn every_plugin_reports_an_unreachable_rpc_as_network_unavailable() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        for plugin in crate::plugins::rails() {
            let network = plugin.test_support().network;
            let transport = plugin.connect(network, &url).unwrap();
            let error = transport.call("getLatestLedger", json!({})).unwrap_err();
            assert_eq!(
                classify(&error),
                Failure::NetworkUnavailable,
                "{}: {}",
                plugin.id(),
                error.message
            );
        }
    }
}
