#!/bin/bash

# Make scripts executable
chmod +x rebuild.sh
chmod +x start.sh
chmod +x stop.sh

# Configuration variables
REPO_DIR="$HOME/elata-eeg"  # Repository directory
CURRENT_USER="$USER"        # Current username

set -e  # Exit on error

# Get absolute path of repository directory (resolves ~)
REPO_PATH=$(realpath "$REPO_DIR")

echo "🚀 Starting setup script..."
echo "📂 Using repository path: $REPO_PATH"
echo "👤 Using username: $CURRENT_USER"

# Update and install dependencies
echo "📦 Installing necessary packages..."
sudo apt update
sudo apt install -y chromium-browser npm curl git build-essential

# Install X11 and LXDE
echo "📦 Installing X11 and LXDE packages..."
sudo apt full-upgrade
sudo apt-get install -y xserver-xorg x11-xserver-utils xinit
sudo apt-get install -y lxde-core lxsession lxpanel pcmanfm

# Install Rust if not installed
if ! command -v cargo &> /dev/null; then
    echo "🦀 Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi

# Call rebuild.sh to build everything
echo "🔧 Building all components..."
./rebuild.sh from-setup

# Create systemd service for the Rust daemon
echo "📝 Creating systemd service for Rust daemon..."
sudo tee /etc/systemd/system/daemon.service > /dev/null <<EOL
[Unit]
Description=Rust Daemon
After=network.target

[Service]
ExecStart=/usr/local/bin/eeg_daemon
Restart=always
User=$CURRENT_USER
Group=$CURRENT_USER
Environment=RUST_LOG=info
WorkingDirectory=$REPO_PATH/daemon
StandardOutput=syslog
StandardError=syslog
SyslogIdentifier=rust_daemon

[Install]
WantedBy=multi-user.target
EOL

# Enable and start Rust daemon service
sudo systemctl daemon-reload
sudo systemctl enable daemon
sudo systemctl start daemon

# Note: Next.js kiosk setup is now handled by rebuild.sh

# Create systemd service for Next.js app
echo "📝 Creating systemd service for Next.js..."
sudo tee /etc/systemd/system/kiosk.service > /dev/null <<EOL
[Unit]
Description=Next.js Kiosk
After=network.target

[Service]
ExecStart=/usr/bin/npm start --prefix $REPO_PATH/kiosk
Restart=always
User=$CURRENT_USER
Group=$CURRENT_USER
Environment=NODE_ENV=production
WorkingDirectory=$REPO_PATH/kiosk
StandardOutput=syslog
StandardError=syslog
SyslogIdentifier=nextjs_kiosk

[Install]
WantedBy=multi-user.target
EOL

# Enable and start Next.js service
sudo systemctl daemon-reload
sudo systemctl enable kiosk
sudo systemctl start kiosk

# Configure Chromium kiosk mode
echo "🖥️ Configuring Chromium for kiosk mode..."

# Configure for X11/LXDE
mkdir -p ~/.config/lxsession/LXDE-pi
cat > ~/.config/lxsession/LXDE-pi/autostart <<EOL
@lxpanel --profile LXDE-pi
@pcmanfm --desktop --profile LXDE-pi

# Start Chromium in kiosk mode
@chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito http://localhost:3000
EOL
chmod +x ~/.config/lxsession/LXDE-pi/autostart

# Create .xinitrc file for X11
cat > ~/.xinitrc <<EOL
#!/bin/sh
exec startlxde
EOL
chmod +x ~/.xinitrc

# Enable auto-login
echo "🔑 Enabling auto-login for user '$CURRENT_USER'..."
sudo mkdir -p /etc/systemd/system/getty@tty1.service.d
sudo tee /etc/systemd/system/getty@tty1.service.d/autologin.conf > /dev/null <<EOL
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin $CURRENT_USER --noclear %I 38400 linux
EOL

# Configure X11 to start on boot
echo "🖥️ Configuring X11 to start automatically..."
tee -a ~/.bash_profile > /dev/null <<EOL
if [ -z "\$DISPLAY" ] && [ "\$(tty)" = "/dev/tty1" ]; then
    startx
fi
[ -f "\$HOME/.bashrc" ] && source "\$HOME/.bashrc"
echo "bash_profile was sourced"
if [ -z "\$DISPLAY" ] && [ "\$(tty)" = "/dev/tty1" ]; then
    startx
fi
EOL

echo "🎉 Setup complete! Rebooting..."
sudo reboot
