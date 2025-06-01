import yargs from 'yargs';
import { hideBin } from 'yargs/helpers';
import { DiscoveryService } from './discovery';

// Parse command line arguments and start discovery service
const args = yargs(hideBin(process.argv))
  .option('port', {
    type: 'number',
    default: 5354,
    description: 'UDP listening port'
  })
  .option('ws-port', {
    type: 'number',
    default: 8080,
    description: 'WebSocket service port'
  })
  .option('secret', {
    type: 'string',
    default: 'your-secret-key',
    description: 'HMAC verification secret key'
  })
  .parseSync();

// Create and start discovery service
const discoveryService = new DiscoveryService({
  secret: args.secret as string,
  port: args.port as number,
  wsPort: args['ws-port'] as number
});

discoveryService.start();

// Handle graceful shutdown
process.on('SIGINT', () => {
  console.log('\n🛑 Stopping service...');
  discoveryService.stop();
  process.exit(0);
});

process.on('SIGTERM', () => {
  discoveryService.stop();
  process.exit(0);
});