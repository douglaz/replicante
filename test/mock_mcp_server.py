#!/usr/bin/env python3
"""
Mock MCP Server for testing the Replicante MCP client implementation.
Implements a simple MCP server that responds to JSON-RPC requests via stdio.
"""

import sys
import json
import logging
from typing import Dict, Any, Optional

# Configure logging to stderr so it doesn't interfere with stdout
logging.basicConfig(
    level=logging.DEBUG,
    format='[Mock MCP] %(asctime)s - %(levelname)s - %(message)s',
    stream=sys.stderr
)

class MockMCPServer:
    def __init__(self):
        self.initialized = False
        self.tools = [
            {
                "name": "echo",
                "description": "Echoes back the input",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "message": {"type": "string"}
                    },
                    "required": ["message"]
                }
            },
            {
                "name": "add",
                "description": "Adds two numbers",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "a": {"type": "number"},
                        "b": {"type": "number"}
                    },
                    "required": ["a", "b"]
                }
            },
            {
                "name": "get_time",
                "description": "Gets the current time",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }
        ]
    
    def handle_request(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Handle a JSON-RPC request and return a response."""
        method = request.get("method")
        params = request.get("params", {})
        request_id = request.get("id")
        
        logging.debug(f"Handling request: {method}")
        
        if method == "initialize":
            return self.handle_initialize(request_id, params)
        elif method == "initialized":
            # This is a notification, no response needed
            self.initialized = True
            logging.info("Server initialized")
            return None
        elif method == "tools/list":
            return self.handle_tools_list(request_id, params)
        elif method == "tools/call":
            return self.handle_tool_call(request_id, params)
        else:
            return self.error_response(request_id, -32601, f"Method not found: {method}")
    
    def handle_initialize(self, request_id: Any, params: Dict[str, Any]) -> Dict[str, Any]:
        """Handle the initialize request."""
        logging.info(f"Initialize request from client: {params.get('client_info', {})}")
        
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "mock-mcp-server",
                    "version": "1.0.0"
                },
                "capabilities": {
                    "tools": {
                        "listChanged": True
                    }
                }
            }
        }
    
    def handle_tools_list(self, request_id: Any, params: Dict[str, Any]) -> Dict[str, Any]:
        """Handle the tools/list request."""
        logging.info("Listing available tools")
        
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "tools": self.tools
            }
        }
    
    def handle_tool_call(self, request_id: Any, params: Dict[str, Any]) -> Dict[str, Any]:
        """Handle a tool call request."""
        tool_name = params.get("name")
        arguments = params.get("arguments", {})
        
        logging.info(f"Tool call: {tool_name} with args: {arguments}")
        
        if tool_name == "echo":
            message = arguments.get("message", "")
            result = {
                "content": [
                    {
                        "type": "text",
                        "text": f"Echo: {message}"
                    }
                ],
                "is_error": False
            }
        elif tool_name == "add":
            a = arguments.get("a", 0)
            b = arguments.get("b", 0)
            result = {
                "content": [
                    {
                        "type": "text",
                        "text": f"Result: {a + b}"
                    }
                ],
                "is_error": False
            }
        elif tool_name == "get_time":
            from datetime import datetime
            result = {
                "content": [
                    {
                        "type": "text",
                        "text": f"Current time: {datetime.now().isoformat()}"
                    }
                ],
                "is_error": False
            }
        else:
            return self.error_response(request_id, -32602, f"Unknown tool: {tool_name}")
        
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": result
        }
    
    def error_response(self, request_id: Any, code: int, message: str) -> Dict[str, Any]:
        """Create an error response."""
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": code,
                "message": message
            }
        }
    
    def run(self):
        """Main server loop - read from stdin, write to stdout."""
        logging.info("Mock MCP server started")
        
        try:
            for line in sys.stdin:
                line = line.strip()
                if not line:
                    continue
                
                try:
                    request = json.loads(line)
                    logging.debug(f"Received: {line}")
                    
                    response = self.handle_request(request)
                    
                    # Only send response if it's not None (notifications don't get responses)
                    if response is not None:
                        response_json = json.dumps(response)
                        print(response_json, flush=True)
                        logging.debug(f"Sent: {response_json}")
                        
                except json.JSONDecodeError as e:
                    logging.error(f"Failed to parse JSON: {e}")
                    error = self.error_response(None, -32700, "Parse error")
                    print(json.dumps(error), flush=True)
                except Exception as e:
                    logging.error(f"Error handling request: {e}")
                    error = self.error_response(None, -32603, str(e))
                    print(json.dumps(error), flush=True)
                    
        except KeyboardInterrupt:
            logging.info("Server shutting down")
        except Exception as e:
            logging.error(f"Server error: {e}")

if __name__ == "__main__":
    server = MockMCPServer()
    server.run()