#!/bin/bash

echo "🚀 Starting Kiosk Mode..."

# Start services
echo "🔄 Starting daemon and kiosk services..."
sudo systemctl start daemon
sudo systemctl start kiosk

# Remove the development mode flag if it exists
if [ -f "$HOME/.kiosk_dev_mode" ]; then
    echo "🗑️ Removing kiosk dev mode flag..."
    rm "$HOME/.kiosk_dev_mode"
fi

# Fix the autostart file to ensure proper startup
echo "📝 Updating autostart file configuration..."
cat > "$HOME/.config/labwc/autostart" <<EOL
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

# Make the autostart file executable
chmod +x "$HOME/.config/labwc/autostart"

# Make sure all panel instances are killed
echo "🔄 Checking for duplicate panels..."
PANEL_COUNT=$(pgrep -f wf-panel-pi | wc -l)
if [ "$PANEL_COUNT" -gt 1 ]; then
    echo "Detected $PANEL_COUNT panel instances. Fixing..."
    
    # Kill all panel instances
    pkill -9 -f wf-panel-pi || true
    sleep 1  # Give it time to terminate
    
    # Start a single panel instance
    /usr/bin/wf-panel-pi &
    
    echo "Panel fixed. Now running a single instance."
else
    echo "Panel check OK: $PANEL_COUNT instance running."
fi

# Kill Chromium if it's running
pkill -f chromium-browser || true
sleep 1

# Start Chromium directly
echo "Starting Chromium in kiosk mode..."
chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito http://localhost:3000 &

echo "✅ Kiosk mode started!"
echo "ℹ️ For full changes to take effect, consider rebooting with: sudo reboot"