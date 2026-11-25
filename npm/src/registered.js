// The only JavaScript file that names vendors: one line per registered rail plugin.
// A module lists its exports by name, not by line, so each export name starts with the rail's place in the
// registration order. A host takes the first network of the first rail as its default: a new rail goes last.
export {default as rail1Stellar} from '@allowit/stellar/plugin';
export {default as rail2Solana} from '@allowit/solana/plugin';
