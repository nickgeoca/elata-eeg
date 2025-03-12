#!/bin/bash

echo "🛑 Stopping Kiosk Mode..."

# Stop services
sudo systemctl stop daemon
sudo systemctl stop kiosk

# Kill Chromium if still running
pkill -f chromium-browser

# Temporarily disable Chromium in autostart file
if [ -f "$HOME/.config/labwc/autostart" ]; then
    echo "📝 Temporarily disabling Chromium in autostart file..."
    # Create a backup if it doesn't exist
    if [ ! -f "$HOME/.config/labwc/autostart.kiosk.bak" ]; then
        cp "$HOME/.config/labwc/autostart" "$HOME/.config/labwc/autostart.kiosk.bak"
    fi
    
    # Comment out the Chromium line
    sed -i 's/^chromium-browser/#chromium-browser/' "$HOME/.config/labwc/autostart"
    
    # Restart the window manager
    echo "🔄 Restarting window manager..."
    pkill -f labwc
fi

echo "✅ Kiosk mode stopped. You can now develop!"
echo "ℹ️ To restart the desktop environment, run: pkill -f labwc"