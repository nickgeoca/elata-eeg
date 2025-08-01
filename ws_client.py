import asyncio
import websockets

async def test_websocket_server():
    """
    Connects to a WebSocket server, prints a status message, and closes the connection.
    """
    uri = "ws://localhost:9999/ws"
    try:
        async with websockets.connect(uri) as websocket:
            print("Connection successful!")
    except Exception as e:
        print(f"Connection failed: {e}")

if __name__ == "__main__":
    asyncio.run(test_websocket_server())