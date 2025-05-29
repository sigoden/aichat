// This module handles the daemon-specific logic, including the TCP IPC listener.

use std::net::{TcpListener, TcpStream};
use std::io::{self, BufRead, BufReader, Write};
use log::{info, error, warn};

pub fn start_daemon() -> io::Result<()> {
    info!("Starting daemon mode with std::net::TcpListener...");
    let listener = TcpListener::bind("127.0.0.1:8787")?;
    info!("Daemon listening on 127.0.0.1:8787");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                info!("Accepted new connection");
                if let Err(e) = handle_client(stream) {
                    error!("Error handling client: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
    // This part is typically not reached for a server that runs indefinitely
    Ok(())
}

fn handle_client(mut stream: TcpStream) -> io::Result<()> {
    let peer_addr = stream.peer_addr()?;
    info!("Handling client from: {}", peer_addr);

    let mut reader = BufReader::new(stream.try_clone()?); // Clone for separate read/write handling
    let mut writer = stream; // The original stream can be used for writing

    let mut buffer = String::new();
    match reader.read_line(&mut buffer) {
        Ok(0) => {
            warn!("Client {} disconnected before sending data.", peer_addr);
            Ok(()) // Connection closed
        }
        Ok(_) => {
            info!("Received from {}: {}", peer_addr, buffer.trim_end());
            if buffer.trim_end() == "PING" {
                writer.write_all(b"PONG\n")?;
                writer.flush()?;
                info!("Sent PONG to {}", peer_addr);
            } else {
                warn!("Received unexpected message from {}: {}", peer_addr, buffer.trim_end());
                // Optionally send an error message back or just close
                writer.write_all(b"ERROR: Unexpected message\n")?;
                writer.flush()?;
            }
            Ok(())
        }
        Err(e) => {
            error!("Error reading from client {}: {}", peer_addr, e);
            Err(e)
        }
    }
}
