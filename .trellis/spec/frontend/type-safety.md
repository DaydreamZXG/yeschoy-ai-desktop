# Type Safety

> Type safety patterns in this project.

---

## Overview

<!--
Document your project's type safety conventions here.

Questions to answer:
- What type system do you use?
- How are types organized?
- What validation library do you use?
- How do you handle type inference?
-->

(To be filled by the team)

---

## Type Organization

<!-- Where types are defined, shared types vs local types -->

(To be filled by the team)

---

## Validation

<!-- Runtime validation patterns (Zod, Yup, io-ts, etc.) -->

(To be filled by the team)

---

## Common Patterns

<!-- Type utilities, generics, type guards -->

(To be filled by the team)

---

## Forbidden Patterns

<!-- any, type assertions, etc. -->

(To be filled by the team)

## Scenario: Desktop account balance projection

### 1. Scope / Trigger

- Applies whenever the desktop account bridge reads `GET /api/user/self`, constructs `AccountSummary`, validates `AccountProjection`, or formats wallet quota.
- NewAPI intentionally permits wallet quota to become negative when settlement records arrears. A negative balance is valid account data, not a malformed response.

### 2. Signatures

- Server response: `GET /api/user/self` → `data.quota: JSON integer`.
- Native projection: `parse_account(account, status) -> AccountSummary` in `src-tauri/src/account_v2.rs`.
- Native money projection: `account_money(status, balance, consumed) -> AccountMoney` in `src-tauri/src/account_finance.rs`.
- Renderer boundary: `decodeAccountProjection(value, requestId) -> AccountProjection | null`.
- Money boundary: `isAccountMoney(value) -> boolean` and `formatMoney(value, currency) -> string`.
- Display conversion: `quotaToUsd(quota, quotaPerUnit) -> number | null`.

### 3. Contracts

- `data.quota` / `AccountSummary.balanceQuota` is a signed base-10 integer. Negative, zero, and positive values must survive the server → Rust → Tauri → TypeScript round trip unchanged.
- `AccountMoney.balanceAmount` is signed; `consumedAmount` and `displayRate` remain non-negative. The native and renderer money validators must use the same distinction.
- `used_quota`, `request_count`, `consumedQuota`, `requestRate`, and `tokenCount` remain non-negative integer counters.
- `quotaPerUnit` must be positive before balance conversion; invalid or missing conversion metadata renders the amount unavailable without invalidating the authenticated account.
- A transiently rejected or undecodable authorization poll must keep the issued-code context, expose `lastError`, and schedule another bounded poll. A last-known-good `signed_in` projection may remain visible during transient command failures.

### 4. Validation & Error Matrix

- `quota = -1`, `0`, or a positive integer → valid account balance.
- `quota` missing, fractional, string-encoded by the server, or outside the native signed integer range → invalid account response.
- Negative `used_quota` or `request_count` → invalid account response.
- Valid signed balance with `quota_per_unit > 0` → render the signed USD result.
- Authorization polling rejects while the UI is pending → keep the pending authorization context, expose the connection error, and schedule another poll until the flow settles or expires.
- Refresh rejects while a valid signed-in projection is visible → preserve that last-known-good signed-in projection.

### 5. Good / Base / Bad Cases

- Good: `quota: -125000`, `quota_per_unit: 500000` projects as `balanceQuota: "-125000"` and displays `-0.25 USD`.
- Base: `quota: 0` displays `0.00 USD` and remains signed in.
- Bad: `used_quota: -1` or `request_count: -1` fails closed instead of displaying fabricated counters.

### 6. Tests Required

- Rust unit test asserts `parse_account` preserves a negative `quota` and still rejects negative counters.
- Renderer decoder test asserts a complete signed-in projection accepts negative `balanceQuota` and rejects negative `usedQuota` / `requestCount`.
- Native and renderer money tests assert a negative balance converts without allowing negative consumed totals.
- Conversion and account-view tests assert the signed monetary value is visible.
- Hook tests start from `authorization_pending`, reject or transiently fail a poll, and assert that polling resumes and eventually reaches `signed_in` without losing the issued-code context.

### 7. Wrong vs Correct

#### Wrong

```typescript
// Treats a legitimate debt balance as malformed account data.
integerText(account.balanceQuota)
Number(balanceQuota) >= 0
```

#### Correct

```typescript
// Balance is signed; usage and request counters are not.
signedIntegerText(account.balanceQuota)
integerText(account.usedQuota)
integerText(account.requestCount)
```
