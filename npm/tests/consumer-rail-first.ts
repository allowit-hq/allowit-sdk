// A host that loads a rail's declarations before the SDK's. The rail's package names its own Core release; the SDK's
// re-exported Core types must still be the SDK's Core types, whatever TypeScript reads first.
import type * as Rail from '@allowit/stellar/owner-chain';
import type {CatalogEntry, Quantity, Settlement, PaymentLimit, OperationReports, WalletKindInfo} from '../src/registry.js';

type RailModule = typeof Rail;
const five: Quantity = {units: '50000000', asset: {symbol: 'XYZ', decimals: 7, name: 'XYZ', native: true}, text: '5 XYZ'};
const settled: Settlement = {height: 12, fee: five, amount: null};
const perPayment: PaymentLimit = {param: 'payment_limit', limit: five};
const reports: OperationReports = {settlement: settled, paymentLimits: [perPayment]};
const kind: WalletKindInfo['id'] = 'native';
const entry: Pick<CatalogEntry, 'access'> = {access: [{kind: 'free'}]};
void [reports, kind, entry, null as unknown as RailModule];
