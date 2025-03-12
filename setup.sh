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
sudo apt install -y chromium-browser xserver-xorg x11-xserver-utils xinit openbox npm curl git build-essential

# Install Rust if not installed
if ! command -v cargo &> /dev/null; then
    echo "🦀 Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi

# Ensure Rust environment is available in all shell sessions
echo "🔧 Ensuring Rust environment is available in all shell sessions..."
if ! grep -q "export PATH=\"\$HOME/.cargo/bin:\$PATH\"" $HOME/.bashrc; then
    echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> $HOME/.bashrc
fi

# Ensure .bash_profile sources .bashrc for SSH sessions
if [ -f "$HOME/.bash_profile" ] && ! grep -q "source.*bashrc" $HOME/.bash_profile; then
    echo '[ -f "$HOME/.bashrc" ] && source "$HOME/.bashrc"' >> $HOME/.bash_profile
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

# Configure for Openbox (if used)
mkdir -p ~/.config/openbox
echo 'chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito http://localhost:3000' > ~/.config/openbox/autostart
chmod +x ~/.config/openbox/autostart

# Configure for labwc (Raspberry Pi OS default)
mkdir -p ~/.config/labwc
cat > ~/.config/labwc/autostart <<EOL
#!/bin/sh

# Start the default desktop components
/usr/bin/lwrespawn /usr/bin/pcmanfm --desktop --profile LXDE-pi &
/usr/bin/lwrespawn /usr/bin/wf-panel-pi &
/usr/bin/kanshi &

# Start Chromium in kiosk mode
chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito http://localhost:3000 &

# Start the XDG autostart applications
/usr/bin/lxsession-xdg-autostart
EOL
chmod +x ~/.config/labwc/autostart

# Enable auto-login
echo "🔑 Enabling auto-login for user '$CURRENT_USER'..."
sudo mkdir -p /etc/systemd/system/getty@tty1.service.d
sudo tee /etc/systemd/system/getty@tty1.service.d/autologin.conf > /dev/null <<EOL
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin $CURRENT_USER --noclear %I 38400 linux
EOL

# Start X on boot
echo "🖥️ Configuring X to start automatically..."
tee -a ~/.bash_profile > /dev/null <<EOL
if [ -z "\$DISPLAY" ] && [ "\$(tty)" = "/dev/tty1" ]; then
    startx
fi
EOL

echo "🎉 Setup complete! Rebooting..."
sudo reboot
