#!/bin/bash

echo "🛑 Stopping Kiosk Mode..."

# Stop services
sudo systemctl stop daemon
sudo systemctl stop kiosk

# Kill Chromium if still running
pkill -f chromium-browser

# Create a modified autostart file that doesn't start Chromium
echo "📝 Updating autostart file for development mode..."
cat > "$HOME/.config/labwc/autostart" <<EOL
#!/bin/sh

# Start the default desktop components
/usr/bin/lwrespawn /usr/bin/pcmanfm --desktop --profile LXDE-pi &
/usr/bin/lwrespawn /usr/bin/wf-panel-pi &
/usr/bin/kanshi &

# Chromium is disabled in development mode
# chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito http://localhost:3000 &

# Start the XDG autostart applications
/usr/bin/lxsession-xdg-autostart
EOL

# Make the autostart file executable
chmod +x "$HOME/.config/labwc/autostart"

# Make sure all panel instances are killed
echo "🔄 Killing all panel instances..."
pkill -9 -f wf-panel-pi || true  # Force kill all panel instances
sleep 1  # Give it time to terminate

# Start a single panel instance for development
/usr/bin/wf-panel-pi &

# Create a flag file to indicate we're in development mode
touch "$HOME/.kiosk_dev_mode"

echo "✅ Kiosk mode stopped. You can now develop!"
echo "ℹ️ To apply changes fully, please reboot with: sudo reboot"