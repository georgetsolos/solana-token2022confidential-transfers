# Token-2022 Security Analysis

## Finding SEC-001: Permanent Delegate and Confidential Balances

### Executive Summary

The Permanent Delegate extension provides administrative
authority over ordinary public token balances.

However, this authority does not automatically provide
the cryptographic spending authority required to recover
confidential token balances.

This creates a potential compliance and asset-recovery
gap for regulated token issuers.

### Test Environment

- Network: Localnet
- Solana CLI: 4.1.2
- Token program: SPL Token-2022
- Test file: tests/security_test.rs
- Test: test_permanent_delegate_security_boundary
- Result: PASSED
- Runtime: 16.05 seconds

All accounts and tokens used in the experiment were
synthetic and created exclusively for local testing.

### Threat Model

A previously approved account becomes subject to a
compliance restriction after confidential funds are
already present.

The issuer must determine whether administrative
authority can recover or restrict those funds.

The relevant distinction is between public token
authority, confidential spending authority, and
account-freezing authority.

### Experimental Results

| Operation | Result |
|-----------|--------|
| Permanent Delegate public transfer | Successful |
| Ordinary transfer against confidential balance | Rejected |
| Administrative account freeze | Successful |
| Confidential withdrawal from frozen account | Rejected |
| Confidential balance preservation | Verified |

The initial account contained one token.

The experiment verified that 0.200 public tokens could
be transferred using Permanent Delegate authority.

The treasury received 0.198 tokens after the
0.002-token transfer fee.

The remaining confidential balance was 0.800 tokens.

An ordinary transfer against the remaining confidential
balance was rejected with Token-2022 error 0x1
(Insufficient Funds).

After the administrator froze the account, a confidential
withdrawal was rejected with Token-2022 error 0x11
(Account Is Frozen).

The confidential balance remained unchanged.

### Security Impact

Permanent Delegate authority must not be treated as
universal recovery authority over confidential assets.

Account freezing provides a containment mechanism,
but containment does not itself recover encrypted funds.

The distinction is important for regulated assets,
stablecoins, institutional payments, and tokenized
real-world assets.

### Recommended Mitigations

1. Require manual confidential account approval.
2. Perform identity and compliance checks before approval.
3. Monitor account compliance status continuously.
4. Apply authorized account freezing when necessary.
5. Separate mint, freeze, and delegate authorities.
6. Protect administrative keys using appropriate custody
   and multisignature arrangements.
7. Establish legally authorized recovery procedures.
8. Independently review all confidential instruction paths.
9. Document the limitations of confidential asset recovery.

An auditor key, if configured, may support visibility
but must not be confused with spending authority.

### Limitations

The experiment verifies ordinary public transfers and
confidential withdrawal restrictions.

It does not establish the behavior of every possible
confidential instruction or authority combination.

The test uses small synthetic balances and a local
validator with compatible program binaries.

The rejected withdrawal may leave temporary proof
accounts that require cleanup.

This implementation is an educational demonstration
and has not undergone an independent security audit.

### Conclusion

Permanent Delegate and Confidential Transfer extensions
provide different capabilities.

Regulated token issuers must design their compliance
controls around these boundaries rather than assume
that administrative authority guarantees complete
asset recovery.
