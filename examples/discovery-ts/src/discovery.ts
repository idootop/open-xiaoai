import dgram from 'dgram';
import crypto from 'crypto';
import os from 'os';

export interface DiscoveryOptions {
  secret: string;
  port: number;
  wsPort: number;
}

export class DiscoveryService {
  private readonly secret: Buffer;
  private readonly port: number;
  private readonly wsPort: number;
  private socket: dgram.Socket;
  private isRunning: boolean = false;

  constructor(options: DiscoveryOptions) {
    this.secret = Buffer.from(options.secret, 'utf8');
    this.port = options.port;
    this.wsPort = options.wsPort;
    this.socket = dgram.createSocket('udp4');

    this.socket.on('error', (err) => {
      console.error(`❌ UDP socket error: ${err}`);
      this.stop();
    });

    this.socket.on('message', (msg, rinfo) => {
      this.handleMessage(msg, rinfo);
    });
  }

  start(): void {
    if (this.isRunning) return;

    this.socket.bind(this.port, () => {
      this.socket.setBroadcast(true);
      this.isRunning = true;
      console.log(`🔍 Discovery service listening on UDP/${this.port}, WS port: ${this.wsPort}`);
    });
  }

  stop(): void {
    if (!this.isRunning) return;
    this.isRunning = false;
    this.socket.close();
  }

  private handleMessage(msg: Buffer, rinfo: dgram.RemoteInfo): void {
    if (this.validatePacket(msg)) {
      const response = this.createResponse(msg);
      this.socket.send(response, rinfo.port, rinfo.address);
      console.log(`✅ Responded to discovery request from ${rinfo.address}:${rinfo.port}`);
    } else {
      console.log(`❌ Invalid discovery request from ${rinfo.address}:${rinfo.port}`);
    }
  }

  private validatePacket(data: Buffer): boolean {
    // Packet structure: [deviceId:16][nonce:4][timestamp:8] (28 bytes)
    // Note: HMAC is not sent in request, client will verify it in response
    if (data.length !== 28) return false;

    const timestamp = data.readBigUInt64BE(20);
    // Time window validation (±30 seconds)
    const currentTime = BigInt(Math.floor(Date.now() / 1000));
    return !(currentTime - timestamp > 30n || timestamp - currentTime > 30n);
  }

  private createResponse(request: Buffer): Buffer {
    // Response structure: [request:28][ip:4][wsPort:2][hmac:32] (66 bytes)
    // Server must prove knowledge of key, client has final verification control
    const response = Buffer.alloc(66);
    request.copy(response, 0, 0, 28);

    // Add server IP address (4 bytes)
    const interfaces = os.networkInterfaces();
    const addresses = Object.values(interfaces)
      .flat()
      .filter((i) => i && i.family === 'IPv4' && !i.internal);
    
    if (addresses.length === 0) {
      throw new Error('No available network interface found');
    }

    const ipParts = addresses[0]!.address.split('.').map(Number);
    response.writeUInt8(ipParts[0], 28);
    response.writeUInt8(ipParts[1], 29);
    response.writeUInt8(ipParts[2], 30);
    response.writeUInt8(ipParts[3], 31);

    // Add WebSocket port (2 bytes, big-endian)
    response.writeUInt16BE(this.wsPort, 32);

    // Calculate HMAC for response
    const hmac = crypto.createHmac('sha256', this.secret);
    hmac.update(request.subarray(0, 28)); // deviceId + nonce + timestamp
    hmac.update(response.subarray(28, 34)); // ip + port
    const calculatedHmac = hmac.digest();
    calculatedHmac.copy(response, 34);

    return response;
  }
}