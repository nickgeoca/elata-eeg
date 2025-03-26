# EEG Recordings Download

This feature allows you to browse and download EEG recording files (CSV) from the recordings directory.

## Features

- View a list of all EEG recording files
- Sort recordings by date (newest first)
- See file size and creation date
- Download files directly to your computer

## How to Use

1. Click on the "Recordings" button in the EEG Monitor interface
2. Browse the list of available recordings
3. Click "Download" next to any file to save it to your computer

## HTTPS Support

For security reasons, file downloads should ideally be served over HTTPS in production.

For development, you can use the standard Next.js development server:
```
npm run dev
```

If you need HTTPS in development, you can use tools like:

1. **mkcert** - For creating locally-trusted development certificates:
   ```
   # Install mkcert
   # Then create certificates
   mkcert -install
   mkcert localhost
   
   # Use a tool like local-ssl-proxy to proxy the Next.js server
   npx local-ssl-proxy --source 3001 --target 3000 --cert localhost.pem --key localhost-key.pem
   ```

2. **ngrok** - For creating a secure tunnel to your local server:
   ```
   npx ngrok http 3000
   ```

These approaches allow you to access your development server securely.

## File Format

The downloaded CSV files contain the following columns:

- `timestamp`: Timestamp in microseconds
- `ch1_voltage` to `ch4_voltage`: Voltage values for each channel
- `ch1_raw_sample` to `ch4_raw_sample`: Raw ADC values for each channel

You can open these files in any spreadsheet application or data analysis tool.