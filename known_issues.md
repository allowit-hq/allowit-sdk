# Known issues

## Native exports from older servers

Some servers omit portable administrator source approval from Native public exports.
Those exports cannot prove source consent on another device.
Use `policy export` from the workspace that completed activation, or export again from a server that includes `sourceApproval`.
An address alone cannot recover reviewed source or its approved parameters.
Policy-wallet imports verify the installed source, parameters, code and roles through independent RPC.
