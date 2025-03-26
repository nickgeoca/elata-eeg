import { NextRequest, NextResponse } from 'next/server';
import fs from 'fs';
import path from 'path';

// This function gets all recordings from the recordings directory
export async function GET(request: NextRequest) {
  try {
    // Get the recordings directory path (relative to project root)
    const recordingsDir = path.join(process.cwd(), '..', 'recordings');
    
    // Read the directory
    const files = fs.readdirSync(recordingsDir)
      .filter(file => file.endsWith('.csv')) // Only include CSV files
      .map(file => {
        const filePath = path.join(recordingsDir, file);
        const stats = fs.statSync(filePath);
        
        return {
          name: file,
          path: `/api/recordings/download?file=${encodeURIComponent(file)}`,
          size: stats.size,
          created: stats.birthtime,
        };
      })
      .sort((a, b) => {
        // Sort by creation date (newest first)
        return new Date(b.created).getTime() - new Date(a.created).getTime();
      });
    
    return NextResponse.json({ files });
  } catch (error) {
    console.error('Error reading recordings directory:', error);
    return NextResponse.json(
      { error: 'Failed to read recordings directory' },
      { status: 500 }
    );
  }
}