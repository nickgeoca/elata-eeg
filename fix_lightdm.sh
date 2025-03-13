#!/bin/bash

echo "🔧 LightDM Configuration Fix Script"
echo "=================================="

# Check if running as root
if [ "$EUID" -ne 0 ]; then
  echo "⚠️ This script needs to be run as root to modify LightDM configuration."
  echo "Please run with sudo: sudo ./fix_lightdm.sh"
  exit 1
fi

LIGHTDM_CONF="/etc/lightdm/lightdm.conf"

# Check if LightDM config exists
if [ ! -f "$LIGHTDM_CONF" ]; then
  echo "❌ Error: LightDM configuration file not found at $LIGHTDM_CONF"
  exit 1
fi

# Create backup of current config
BACKUP_FILE="${LIGHTDM_CONF}.backup.$(date +%Y%m%d%H%M%S)"
cp "$LIGHTDM_CONF" "$BACKUP_FILE"
echo "✅ Created backup of current configuration at $BACKUP_FILE"

# Check current configuration
echo "🔍 Current LightDM configuration:"
grep -E 'greeter-session|user-session|autologin-session' "$LIGHTDM_CONF" || echo "  No session settings found"

# Update the configuration
echo "🔄 Updating LightDM configuration..."

# Check if [Seat:*] section exists, add it if not
if ! grep -q "\[Seat:\*\]" "$LIGHTDM_CONF"; then
  echo -e "\n[Seat:*]" >> "$LIGHTDM_CONF"
  echo "  Added [Seat:*] section"
fi

# Function to update or add a setting
update_setting() {
  local setting=$1
  local value=$2
  local comment=$3
  
  # If setting should be commented out
  if [ "$comment" = "true" ]; then
    # Check if setting exists and is not already commented
    if grep -q "^${setting}=" "$LIGHTDM_CONF"; then
      sed -i "s/^${setting}=.*/#${setting}=/" "$LIGHTDM_CONF"
      echo "  Commented out ${setting}"
    # Check if setting exists but is already commented
    elif grep -q "^#${setting}=" "$LIGHTDM_CONF"; then
      echo "  ${setting} is already commented out"
    else
      # Add commented setting under [Seat:*] section
      sed -i "/\[Seat:\*\]/a #${setting}=" "$LIGHTDM_CONF"
      echo "  Added commented ${setting}"
    fi
  else
    # Check if setting exists (commented or not)
    if grep -q "^#\?${setting}=" "$LIGHTDM_CONF"; then
      # Replace existing setting (commented or not)
      sed -i "s/^#\?${setting}=.*/${setting}=${value}/" "$LIGHTDM_CONF"
      echo "  Updated ${setting}=${value}"
    else
      # Add new setting under [Seat:*] section
      sed -i "/\[Seat:\*\]/a ${setting}=${value}" "$LIGHTDM_CONF"
      echo "  Added ${setting}=${value}"
    fi
  fi
}

# Update settings based on colleague's notes
update_setting "greeter-session" "pi-greeter-labwc" "true"  # Comment out non-existent greeter
update_setting "user-session" "labwc" "false"               # Set user session to labwc
update_setting "autologin-session" "labwc" "false"          # Set autologin session to labwc

# Check if autologin-user is set, add it if not
if ! grep -q "autologin-user=" "$LIGHTDM_CONF"; then
  CURRENT_USER=$(logname || echo "$SUDO_USER" || echo "$USER")
  update_setting "autologin-user" "$CURRENT_USER" "false"
  echo "  Added autologin-user=$CURRENT_USER"
fi

# Display updated configuration
echo "🔍 Updated LightDM configuration:"
grep -E 'greeter-session|user-session|autologin-session|autologin-user' "$LIGHTDM_CONF" || echo "  No session settings found"

echo "✅ LightDM configuration updated successfully!"
echo "ℹ️ To apply changes, restart LightDM with: sudo systemctl restart lightdm"
echo "ℹ️ Or run the enhanced start.sh script which will restart LightDM for you."