#!/usr/bin/env python3
"""
MCP Tools Client for Docker environment
This script acts as a bridge between Replicante and the containerized MCP tools
"""

import sys
import json
import logging
import subprocess
import os

# Configure logging to stderr
logging.basicConfig(
    level=logging.INFO,
    format='[MCP Client] %(asctime)s - %(levelname)s - %(message)s',
    stream=sys.stderr
)

class MCPToolsClient:
    def __init__(self):
        self.initialized = False
        # For Docker environment, we'll use the http_mcp_server directly
        self.server_process = None
    
    def start_server(self):
        """Start the MCP server process"""
        try:
            # Start the HTTP MCP server as a subprocess
            self.server_process = subprocess.Popen([
                "python", "-u", "/app/http_mcp_server.py"
            ], stdin=subprocess.PIPE, stdout=subprocess.PIPE, 
               stderr=subprocess.PIPE, universal_newlines=True, bufsize=1)
            
            logging.info("Started MCP server subprocess")
            return True
        except Exception as e:
            logging.error(f"Failed to start MCP server: {e}")
            return False
    
    def send_to_server(self, message):
        """Send a message to the MCP server and get response"""
        if not self.server_process:
            return None
            
        try:
            # Send message to server
            self.server_process.stdin.write(message + "\n")
            self.server_process.stdin.flush()
            
            # Read response
            response = self.server_process.stdout.readline()
            return response.strip() if response else None
        except Exception as e:
            logging.error(f"Error communicating with server: {e}")
            return None
    
    def handle_request(self, request_line):
        """Handle a JSON-RPC request"""
        try:
            request = json.loads(request_line)
            logging.debug(f"Handling request: {request.get('method')}")
            
            # Start server if needed
            if not self.server_process and not self.start_server():
                return self.error_response(request.get('id'), -32603, "Failed to start MCP server")
            
            # Forward request to server
            response_line = self.send_to_server(request_line)
            if response_line:
                return response_line
            else:
                return self.error_response(request.get('id'), -32603, "No response from server")
                
        except json.JSONDecodeError as e:
            logging.error(f"Failed to parse JSON: {e}")
            return self.error_response(None, -32700, "Parse error")
        except Exception as e:
            logging.error(f"Error handling request: {e}")
            return self.error_response(None, -32603, str(e))
    
    def error_response(self, request_id, code, message):
        """Create an error response"""
        error = {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": code,
                "message": message
            }
        }
        return json.dumps(error)
    
    def run(self):
        """Main client loop"""
        logging.info("MCP Tools Client starting...")
        
        try:
            for line in sys.stdin:
                line = line.strip()
                if not line:
                    continue
                
                response = self.handle_request(line)
                if response:
                    print(response, flush=True)
                    
        except KeyboardInterrupt:
            logging.info("Client shutting down")
        except Exception as e:
            logging.error(f"Client error: {e}")
        finally:
            if self.server_process:
                self.server_process.terminate()
                self.server_process.wait()

if __name__ == "__main__":
    client = MCPToolsClient()
    client.run()