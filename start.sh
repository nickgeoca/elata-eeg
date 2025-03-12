#!/bin/bash

echo "🚀 Starting Kiosk Mode..."

# Start services
echo "🔄 Starting daemon and kiosk services..."
sudo systemctl start daemon
sudo systemctl start kiosk

# Restore Chromium in autostart file if backup exists
if [ -f "$HOME/.config/labwc/autostart.kiosk.bak" ]; then
    echo "📝 Restoring Chromium in autostart file..."
    cp "$HOME/.config/labwc/autostart.kiosk.bak" "$HOME/.config/labwc/autostart"
else
    # If no backup exists, uncomment the Chromium line
    sed -i 's/^#chromium-browser/chromium-browser/' "$HOME/.config/labwc/autostart"
fi

# Restart the window manager to apply changes
echo "🔄 Restarting window manager to apply changes..."
pkill -f labwc

echo "✅ Kiosk mode started!"