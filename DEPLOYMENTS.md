# Deployments

Live contract addresses for the Stellar RWA Toolkit contracts.

## Testnet

Network: `Test SDF Network ; September 2015`
Deployed: 2026-07-08

| Contract    | Contract ID                                                | Explorer |
|-------------|------------------------------------------------------------|----------|
| compliance  | `CBUERYDM7DXTZLLKDBRJKUBPFJ7M4OSUN4T7XKUARU345RLXNAIQD2IU` | [view](https://stellar.expert/explorer/testnet/contract/CBUERYDM7DXTZLLKDBRJKUBPFJ7M4OSUN4T7XKUARU345RLXNAIQD2IU) |
| registry    | `CBX5SMLTXX6JP4HA5GQIO2V6QM7WCUGL2GZ6D4U773HMRI6RXISKPUR3` | [view](https://stellar.expert/explorer/testnet/contract/CBX5SMLTXX6JP4HA5GQIO2V6QM7WCUGL2GZ6D4U773HMRI6RXISKPUR3) |
| dividend    | `CAR4XY3CEBQWFOL27JEWFW34KXSIZA7RFKDQMEIV7ZU723RWY37I2SYX` | [view](https://stellar.expert/explorer/testnet/contract/CAR4XY3CEBQWFOL27JEWFW34KXSIZA7RFKDQMEIV7ZU723RWY37I2SYX) |
| asset-token | `CBMCWLSQSWUTLUJFCNBHNBSXMUM3XU7NAQ5TSNERW4HA4ZZBYHLG4ECZ` | [view](https://stellar.expert/explorer/testnet/contract/CBMCWLSQSWUTLUJFCNBHNBSXMUM3XU7NAQ5TSNERW4HA4ZZBYHLG4ECZ) |

**Admin / issuer account:** `GAIQGTOBTTLLDJ4SWGGESM7UWJ2DI4K3ZNHUSHPDKJL2IE5FKY3BSRAA`

### Sample asset

The deployment script registers one sample asset for demonstration:

- **Name:** Manhattan Loft (`MLOFT`)
- **Type:** `real_estate`
- **Total supply:** 1,000,000 (2 decimals)
- **Valuation:** 500,000,000 USD cents ($5,000,000)
- **Compliance:** gated by the compliance contract above; the admin/issuer is
  the only KYC-approved holder at deploy time.

## Reproducing

```bash
NETWORK=testnet IDENTITY=rwa-admin ./scripts/deploy.sh
```

The script funds a fresh identity via friendbot, builds the wasm, deploys and
initializes all four contracts, approves the issuer on the compliance contract,
deploys a sample asset token, and registers it in the registry. It prints the
resulting contract ids; paste them into this file.

## Mainnet

Not yet deployed.
