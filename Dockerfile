# Use Alpine Linux for the builder stage
FROM rust:1.74-alpine AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev openssl-dev

# Set the working directory in the container
WORKDIR /usr/src/app

# Copy the current directory contents into the container
COPY . .

# Build the application
RUN cargo build --release

# Use Alpine Linux for the final stage
FROM alpine:3.18

# Install runtime dependencies
RUN apk add --no-cache libgcc openssl ca-certificates

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
ENTRYPOINT ["/usr/local/bin/aichat"]
