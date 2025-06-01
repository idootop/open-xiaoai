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
    // Packet structure: [deviceId:16][nonce:4][timestamp:8][hmac:32]
    if (data.length !== 60) return false;

    const deviceId = data.subarray(0, 16);
    const nonce = data.subarray(16, 20);
    const timestamp = data.readBigUInt64BE(20);
    const receivedHmac = data.subarray(28, 60);

    // Time window validation (±30 seconds)
    const currentTime = BigInt(Math.floor(Date.now() / 1000));
    if (currentTime - timestamp > 30n || timestamp - currentTime > 30n) {
      return false;
    }

    // Calculate HMAC
    const hmac = crypto.createHmac('sha256', this.secret);
    hmac.update(deviceId);
    hmac.update(nonce);
    hmac.update(data.subarray(20, 28)); // timestamp bytes

    const calculatedHmac = hmac.digest();
    return crypto.timingSafeEqual(calculatedHmac, receivedHmac);
  }

  private createResponse(request: Buffer): Buffer {
    // Response structure: [first 32 bytes of request][ip:4][wsPort:2]
    const response = Buffer.alloc(38);
    request.copy(response, 0, 0, 32);

    // Add server IP address (4 bytes)
    const interfaces = os.networkInterfaces();
    const addresses = Object.values(interfaces)
      .flat()
      .filter((i) => i && i.family === 'IPv4' && !i.internal);
    
    if (addresses.length === 0) {
      throw new Error('No available network interface found');
    }

    const ipParts = addresses[0]!.address.split('.').map(Number);
    response.writeUInt8(ipParts[0], 32);
    response.writeUInt8(ipParts[1], 33);
    response.writeUInt8(ipParts[2], 34);
    response.writeUInt8(ipParts[3], 35);

    // Add WebSocket port (2 bytes, big-endian)
    response.writeUInt16BE(this.wsPort, 36);

    return response;
  }
}