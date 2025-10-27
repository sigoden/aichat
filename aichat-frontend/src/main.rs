use clap::Parser;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use log::{info, error, warn};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(long)]
    daemon: bool,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    if cli.daemon {
        // Initialize env_logger. RUST_LOG can be used to set log level, e.g., RUST_LOG=info
        env_logger::Builder::from_default_env()
            .format_timestamp_millis()
            .init();

        info!("Starting aichat-frontend in daemon mode...");
        start_daemon()?;
    } else {
        println!("aichat-frontend: Run with --daemon to start the server.");
        println!("Example: target/debug/aichat-frontend --daemon");
    }

    Ok(())
}

fn start_daemon() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8788")?;
    info!("Daemon listening on 127.0.0.1:8788");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let peer_addr = stream.peer_addr().map_or_else(|_| "unknown".to_string(), |addr| addr.to_string());
                info!("Accepted new connection from {}", peer_addr);
                if let Err(e) = handle_client(stream) {
                    error!("Error handling client {}: {}", peer_addr, e);
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
    let peer_addr = stream.peer_addr().map_or_else(|_| "unknown".to_string(), |addr| addr.to_string());
    
    // Use try_clone to get a readable handle (reader) and a writable handle (writer)
    // This is important because BufReader might buffer data that then becomes unavailable to the writer if they share the same underlying stream directly without careful management.
    let stream_clone = stream.try_clone()?;
    let mut reader = BufReader::new(stream_clone);
    // The original stream can be used for writing.
    let mut writer = stream;

    let mut buffer = String::new();
    match reader.read_line(&mut buffer) {
        Ok(0) => {
            warn!("Client {} disconnected before sending data.", peer_addr);
            // No data received, connection closed by peer
        }
        Ok(_) => {
            let received_msg = buffer.trim_end();
            info!("Received from {}: {}", peer_addr, received_msg);
            if received_msg == "PING" {
                writer.write_all(b"PONG\n")?;
                writer.flush()?; // Ensure the response is sent immediately
                info!("Sent PONG to {}", peer_addr);
            } else {
                warn!("Received unexpected message from {}: {}", peer_addr, received_msg);
                let error_msg = format!("ERROR: Unexpected message '{}'\n", received_msg);
                writer.write_all(error_msg.as_bytes())?;
                writer.flush()?;
            }
        }
        Err(e) => {
            error!("Error reading from client {}: {}", peer_addr, e);
            // Return the error to the main loop to log it against the client connection
            return Err(e);
        }
    }
    Ok(())
}
