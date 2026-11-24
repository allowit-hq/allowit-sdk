// The only JavaScript file that names vendors: one line per registered rail plugin.
export {default as stellar} from '@allowit/stellar/plugin';
export {default as solana} from '@allowit/solana/plugin';
// The registration order. A module lists its exports by name, so the order is stated here: a host takes the first
// network of the first rail as its default.
export const order = Object.freeze(['stellar', 'solana']);
