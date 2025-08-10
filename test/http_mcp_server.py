#!/usr/bin/env python3
"""
HTTP MCP Server for testing - provides tools for web requests
"""

import sys
import json
import logging
from typing import Dict, Any
from urllib.request import urlopen, Request
from urllib.parse import urlparse
from datetime import datetime

# Configure logging to stderr
logging.basicConfig(
    level=logging.INFO,
    format='[HTTP MCP] %(asctime)s - %(levelname)s - %(message)s',
    stream=sys.stderr
)

class HttpMCPServer:
    def __init__(self):
        self.initialized = False
        self.tools = [
            {
                "name": "fetch_url",
                "description": "Fetch content from a URL",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "The URL to fetch"}
                    },
                    "required": ["url"]
                }
            },
            {
                "name": "check_weather",
                "description": "Get current weather (mock data)",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string", "description": "City name"}
                    },
                    "required": ["city"]
                }
            },
            {
                "name": "get_time",
                "description": "Get current time in various timezones",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "timezone": {"type": "string", "description": "Timezone (e.g., UTC, EST, PST)"}
                    }
                }
            },
            {
                "name": "calculate",
                "description": "Perform basic calculations",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "expression": {"type": "string", "description": "Math expression to evaluate"}
                    },
                    "required": ["expression"]
                }
            }
        ]
    
    def handle_request(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Handle JSON-RPC request."""
        method = request.get("method")
        params = request.get("params", {})
        request_id = request.get("id")
        
        logging.debug(f"Handling request: {method}")
        
        # Handle different methods
        if method == "initialize":
            return self.handle_initialize(request_id, params)
        elif method == "initialized":
            # Notification, no response needed
            self.initialized = True
            logging.info("Client confirmed initialization")
            return None
        elif method == "tools/list":
            return self.handle_tools_list(request_id)
        elif method == "tools/call":
            return self.handle_tool_call(request_id, params)
        else:
            return self.error_response(request_id, -32601, f"Method not found: {method}")
    
    def handle_initialize(self, request_id: Any, params: Dict[str, Any]) -> Dict[str, Any]:
        """Handle initialize request."""
        client_info = params.get("client_info", {})
        logging.info(f"Initialize request from client: {client_info}")
        
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "protocol_version": "2024-11-05",
                "server_info": {
                    "name": "http-mcp-server",
                    "version": "1.0.0"
                },
                "capabilities": {
                    "tools": {
                        "list_changed": True
                    }
                }
            }
        }
    
    def handle_tools_list(self, request_id: Any) -> Dict[str, Any]:
        """Return list of available tools."""
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "tools": self.tools
            }
        }
    
    def handle_tool_call(self, request_id: Any, params: Dict[str, Any]) -> Dict[str, Any]:
        """Execute a tool and return the result."""
        tool_name = params.get("name")
        arguments = params.get("arguments", {})
        
        logging.info(f"Tool call: {tool_name} with args: {arguments}")
        
        # Execute the tool
        if tool_name == "fetch_url":
            result = self.fetch_url(arguments)
        elif tool_name == "check_weather":
            result = self.check_weather(arguments)
        elif tool_name == "get_time":
            result = self.get_time(arguments)
        elif tool_name == "calculate":
            result = self.calculate(arguments)
        else:
            return self.error_response(request_id, -32602, f"Unknown tool: {tool_name}")
        
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": result
        }
    
    def fetch_url(self, args: Dict[str, Any]) -> Dict[str, Any]:
        """Fetch content from a URL."""
        url = args.get("url", "")
        
        try:
            # Only fetch URLs from safe domains for testing
            safe_domains = ["httpbin.org", "jsonplaceholder.typicode.com", "api.github.com"]
            parsed = urlparse(url)
            
            if not any(domain in parsed.netloc for domain in safe_domains):
                return {
                    "content": [{
                        "type": "text",
                        "text": f"Error: URL domain '{parsed.netloc}' not in safe list for testing"
                    }],
                    "is_error": True
                }
            
            req = Request(url, headers={"User-Agent": "MCP-Test/1.0"})
            with urlopen(req, timeout=5) as response:
                content = response.read().decode('utf-8')
                status = response.getcode()
            
            return {
                "content": [{
                    "type": "text",
                    "text": f"Status: {status}\nContent (first 500 chars):\n{content[:500]}"
                }],
                "is_error": False
            }
        except Exception as e:
            return {
                "content": [{
                    "type": "text",
                    "text": f"Error fetching URL: {str(e)}"
                }],
                "is_error": True
            }
    
    def check_weather(self, args: Dict[str, Any]) -> Dict[str, Any]:
        """Return mock weather data."""
        city = args.get("city", "Unknown")
        
        # Mock weather data
        import random
        temp = random.randint(10, 30)
        conditions = random.choice(["Sunny", "Cloudy", "Rainy", "Partly Cloudy"])
        
        return {
            "content": [{
                "type": "text",
                "text": f"Weather in {city}: {temp}°C, {conditions}"
            }],
            "is_error": False
        }
    
    def get_time(self, args: Dict[str, Any]) -> Dict[str, Any]:
        """Get current time in specified timezone."""
        timezone = args.get("timezone", "UTC")
        
        # Simple mock implementation
        from datetime import datetime, timezone as tz, timedelta
        
        now = datetime.now(tz.utc)
        
        # Simple timezone offsets
        offsets = {
            "UTC": 0,
            "EST": -5,
            "PST": -8,
            "CET": 1,
            "JST": 9
        }
        
        offset_hours = offsets.get(timezone.upper(), 0)
        adjusted_time = now + timedelta(hours=offset_hours)
        
        return {
            "content": [{
                "type": "text",
                "text": f"Current time in {timezone}: {adjusted_time.strftime('%Y-%m-%d %H:%M:%S')}"
            }],
            "is_error": False
        }
    
    def calculate(self, args: Dict[str, Any]) -> Dict[str, Any]:
        """Evaluate a math expression."""
        expression = args.get("expression", "")
        
        try:
            # Safe evaluation for basic math
            import ast
            import operator
            
            # Define safe operations
            ops = {
                ast.Add: operator.add,
                ast.Sub: operator.sub,
                ast.Mult: operator.mul,
                ast.Div: operator.truediv,
                ast.Pow: operator.pow,
                ast.Mod: operator.mod,
            }
            
            def eval_expr(node):
                if isinstance(node, ast.Num):
                    return node.n
                elif isinstance(node, ast.BinOp):
                    return ops[type(node.op)](eval_expr(node.left), eval_expr(node.right))
                elif isinstance(node, ast.UnaryOp):
                    return ops[type(node.op)](eval_expr(node.operand))
                else:
                    raise ValueError(f"Unsupported expression: {ast.dump(node)}")
            
            tree = ast.parse(expression, mode='eval')
            result = eval_expr(tree.body)
            
            return {
                "content": [{
                    "type": "text",
                    "text": f"{expression} = {result}"
                }],
                "is_error": False
            }
        except Exception as e:
            return {
                "content": [{
                    "type": "text",
                    "text": f"Error evaluating expression: {str(e)}"
                }],
                "is_error": True
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
        """Main server loop."""
        logging.info("HTTP MCP server started")
        
        try:
            for line in sys.stdin:
                line = line.strip()
                if not line:
                    continue
                
                try:
                    request = json.loads(line)
                    logging.debug(f"Received: {line}")
                    
                    response = self.handle_request(request)
                    
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
    server = HttpMCPServer()
    server.run()