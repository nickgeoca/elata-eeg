#!/bin/bash

# Generate self-signed certificates for HTTPS development
echo "Generating self-signed certificates for HTTPS development..."

# Generate private key and certificate
openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 365 \
  -subj '/CN=localhost' \
  -keyout localhost-key.pem \
  -out localhost-cert.pem

# Check if certificates were created successfully
if [ -f "localhost-key.pem" ] && [ -f "localhost-cert.pem" ]; then
  echo "Certificates generated successfully!"
  echo "  - localhost-key.pem"
  echo "  - localhost-cert.pem"
  
  # Make the script executable
  chmod +x generate-certs.sh
  
  echo ""
  echo "You can now run the Next.js server with HTTPS:"
  echo "  npm run dev"
  echo ""
  echo "Note: Since these are self-signed certificates, you'll need to accept the security warning in your browser."
else
  echo "Failed to generate certificates."
  exit 1
fi