#!/bin/bash
set -e  # Exit on error

# Check if script is being called from setup.sh
FROM_SETUP=${1:-"false"}

echo "🚀 Starting full rebuild process..."

# Stop services and exit kiosk mode (unless called from setup.sh)
if [ "$FROM_SETUP" != "from-setup" ]; then
  echo "🛑 Stopping services and exiting kiosk mode..."
  ./stop.sh
fi

# Rebuild Rust driver
echo "🔧 Rebuilding Rust driver..."
cd driver
cargo clean  # Ensures a fresh build
cargo build --release
cd ..
echo "✅ Driver rebuild complete!"

# Rebuild Rust daemon
echo "🔧 Rebuilding Rust daemon..."
cd daemon
cargo clean  # Ensures a fresh build
cargo build --release
sudo mv target/release/eeg_daemon /usr/local/bin/
cd ..
echo "✅ Daemon rebuild complete!"

# Rebuild kiosk
echo "🧹 Cleaning Next.js build cache..."
cd kiosk
rm -rf .next
  
echo "⚙️ Rebuilding Next.js app..."
npm run build
cd ..
echo "✅ Kiosk rebuild complete!"

# Start services and kiosk mode (unless called from setup.sh)
if [ "$FROM_SETUP" != "from-setup" ]; then
  echo "🚀 Starting services and kiosk mode..."
  ./start.sh
fi

echo "🎉 Rebuild process complete!"
