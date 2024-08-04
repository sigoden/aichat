# Use a more recent version of the official Rust image as a parent image
FROM rust:1.74 AS builder

# Set the working directory in the container
WORKDIR /usr/src/app

# Copy the current directory contents into the container
COPY . .

# Build the application
RUN cargo build --release

# Use a more recent base image for the final image
FROM debian:bookworm-slim

# Install any needed packages
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the built executable from the builder stage
COPY --from=builder /usr/src/app/target/release/aichat /usr/local/bin/aichat

# Create directories for aichat configuration and data
RUN mkdir -p /root/.config/aichat

# Set the working directory
WORKDIR /root

# Create volume mount points for persistent data
VOLUME ["/root/.config/aichat"]

# Expose port 8000 for the server
EXPOSE 8000

# Set the entrypoint to directly call aichat
ENTRYPOINT ["aichat"]
