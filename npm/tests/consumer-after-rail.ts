// The SDK's types as a host reads them after a registered rail's declarations (see types.test.mjs). A rail's package
// names its own Core release; the SDK's re-exported Core types must still be the types of the SDK's Core.
import type {CatalogEntry, Quantity, Settlement, PaymentLimit, OperationReports, WalletKindInfo} from '../src/registry.js';

const five: Quantity = {units: '50000000', asset: {symbol: 'XYZ', decimals: 7, name: 'XYZ', native: true}, text: '5 XYZ'};
const settled: Settlement = {height: 12, fee: five, amount: null};
const perPayment: PaymentLimit = {param: 'payment_limit', limit: five};
const reports: OperationReports = {settlement: settled, paymentLimits: [perPayment], explorer: null};
const kind: WalletKindInfo['id'] = 'native';
const entry: Pick<CatalogEntry, 'access'> = {access: [{kind: 'free'}]};
void [reports, kind, entry];
