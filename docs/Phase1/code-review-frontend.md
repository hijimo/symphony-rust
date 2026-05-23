# Web Frontend Code Review

**Project**: symphony-web (web-frontend/)  
**Date**: 2026-05-20  
**Stack**: React 18 + TypeScript + MUI 6 + Zustand 5 + Vite 6 + Vitest + MSW

---

## Summary

Overall the codebase is well-structured for a Phase 1 project. Architecture is clean, TypeScript is used with strict mode, tests cover critical paths, and accessibility basics are in place. The issues below are ordered by severity.

---

## Critical Issues

### 1. Auth store initializer does not handle malformed JSON in localStorage

**File**: `src/store/auth.ts`, line 15-17  
**Severity**: Critical

```typescript
user: (() => {
  const raw = localStorage.getItem('user');
  return raw ? (JSON.parse(raw) as UserInfo) : null;
})(),
```

If `localStorage.user` contains invalid JSON (corrupted, manually edited, or from a different app version), `JSON.parse` throws and the entire store fails to initialize, crashing the app on load.

**Fix**: Wrap in try/catch, return `null` on parse failure, and clear the corrupted entry:

```typescript
user: (() => {
  const raw = localStorage.getItem('user');
  if (!raw) return null;
  try {
    return JSON.parse(raw) as UserInfo;
  } catch {
    localStorage.removeItem('user');
    localStorage.removeItem('token');
    return null;
  }
})(),
```

---

### 2. Client-side role check is the only admin authorization gate

**File**: `src/components/ProtectedRoute.tsx`, line 17  
**Severity**: Critical (architecture note)

```typescript
if (requireAdmin && user?.role !== 'admin') {
  return <Navigate to="/settings" replace />;
}
```

The `user` object is stored in localStorage and can be trivially modified by the user (e.g., changing `role` from `"user"` to `"admin"`). This is acceptable only if the backend independently enforces admin authorization on all `/admin/*` endpoints. Verify that the backend returns 403 for non-admin tokens on admin routes. If it does, this is informational; if not, it is a real privilege escalation vulnerability.

**Fix**: Ensure backend enforces role checks. Optionally, validate the role claim from the JWT on the client side rather than trusting localStorage.

---

## High Severity Issues

### 3. Token stored in localStorage is vulnerable to XSS

**File**: `src/store/auth.ts`, lines 22-23; `src/api/client.ts`, line 13  
**Severity**: High

JWT tokens in localStorage are accessible to any JavaScript running on the page. If an XSS vulnerability exists anywhere in the app (or in a third-party dependency), the token can be exfiltrated.

**Fix**: Consider using httpOnly cookies for token transport (requires backend changes). If localStorage must be used, ensure a strict Content Security Policy is deployed and all user-generated content is sanitized.

---

### 4. Duplicate type definitions for API response

**File**: `src/types/index.ts` (line 3, `ResponseData<T>`) and `src/api/types.ts` (line 14, `ApiResponse<T>`)  
**Severity**: High (code quality / maintainability)

Two identical interfaces exist for the same API response envelope:
- `ResponseData<T>` in `src/types/index.ts`
- `ApiResponse<T>` in `src/api/types.ts`

This creates confusion about which to use and risks them drifting apart.

**Fix**: Keep one canonical type (e.g., `ApiResponse` in `src/api/types.ts`) and re-export or import it everywhere. Remove the duplicate.

---

### 5. No token expiration check on the client

**File**: `src/store/auth.ts`, `src/api/client.ts`  
**Severity**: High

The login response includes `expiresAt` but it is never stored or checked. The app relies entirely on the backend returning `AUTH_001` to detect expiration. This means users can interact with the UI for an extended period with an expired token, only to be abruptly redirected on the next API call.

**Fix**: Store `expiresAt`, check it before API calls (or on a timer), and proactively redirect to login when the token is about to expire.

---

## Medium Severity Issues

### 6. `window.location.href = '/login'` causes full page reload

**File**: `src/api/client.ts`, lines 26, 43  
**Severity**: Medium

Using `window.location.href` for navigation bypasses React Router, causing a full page reload and loss of any in-memory state. This also makes the behavior untestable without mocking `window.location`.

**Fix**: Use a shared event emitter or a store action that the router listens to, or inject the router's `navigate` function into the interceptor.

---

### 7. DataTable uses array index as React key

**File**: `src/components/DataTable.tsx`, line 141  
**Severity**: Medium

```typescript
{data.map((row, idx) => (
  <TableRow key={idx} ...>
```

Using array index as key can cause incorrect DOM reuse when rows are reordered, deleted, or inserted (e.g., after deleting a user and re-fetching).

**Fix**: Accept a `rowKey` prop (e.g., a function or field name) and use it to derive stable keys:

```typescript
<TableRow key={rowKey ? rowKey(row) : idx} ...>
```

---

### 8. Settings page has excessive local state - no form library

**File**: `src/pages/Settings.tsx`  
**Severity**: Medium (maintainability)

The Settings page manages 15+ `useState` calls for three independent forms. This makes the component hard to maintain and error-prone as forms grow.

**Fix**: Extract each form section into its own component, or adopt a lightweight form library (e.g., react-hook-form) to reduce boilerplate and centralize validation.

---

### 9. `catch (err: any)` used throughout - loses type safety

**Files**: `src/pages/Settings.tsx` (lines 65, 86, 109, 138), `src/pages/AdminUsers.tsx` (lines 84, 146, 181, 203)  
**Severity**: Medium

Using `any` in catch blocks bypasses TypeScript's type checking. The pattern `err?.message` is fragile.

**Fix**: Use `unknown` and narrow with `instanceof Error`:

```typescript
} catch (err: unknown) {
  const message = err instanceof Error ? err.message : '操作失败';
  showSnack(message, 'error');
}
```

---

### 10. PasswordField overwrites parent's slotProps

**File**: `src/components/PasswordField.tsx`, lines 18-33  
**Severity**: Medium

The component spreads `{...props}` but then unconditionally sets `slotProps.input.endAdornment`. If a parent passes custom `slotProps`, they are silently overwritten.

**Fix**: Merge the parent's slotProps with the toggle adornment, or document that `slotProps.input` is reserved.

---

### 11. No error boundary at the app level

**File**: `src/App.tsx`  
**Severity**: Medium

If any component throws during render, the entire app crashes to a white screen. There is no `ErrorBoundary` to catch and display a fallback UI.

**Fix**: Add a top-level `ErrorBoundary` component that shows a user-friendly error page with a "reload" button.

---

### 12. Sidebar `isActive` uses `startsWith` which can false-match

**File**: `src/components/Sidebar.tsx`, line 70  
**Severity**: Medium

```typescript
const isActive = (path: string) => location.pathname.startsWith(path);
```

`/settings` would match `/settings-advanced` if such a route were added. For `/admin/config` vs `/admin/configs`, this could also false-match.

**Fix**: Use exact match or match against the route pattern:

```typescript
const isActive = (path: string) => location.pathname === path || location.pathname.startsWith(path + '/');
```

---

## Low Severity Issues

### 13. Missing `aria-label` on the search TextField in AdminUsers

**File**: `src/pages/AdminUsers.tsx`, line 291  
**Severity**: Low

The search field uses `aria-label="搜索用户"` which is good, but the `<FormControl>` for role filter (line 302) lacks an accessible description connecting the label to the select for screen readers. MUI handles this via `InputLabel` + `Select` pairing, so this is acceptable but worth verifying with a screen reader.

---

### 14. No loading/disabled state on logout button during navigation

**File**: `src/components/TopNav.tsx`, line 36-39  
**Severity**: Low

`handleLogout` calls `logout()` then `navigate()` synchronously. If logout involved an async server call (e.g., token revocation), the button would need a loading state. Currently fine, but worth noting for future changes.

---

### 15. Hardcoded pagination options

**File**: `src/components/DataTable.tsx`, line 161  
**Severity**: Low

`rowsPerPageOptions={[10, 25, 50]}` is hardcoded. Consider making it a prop for reusability.

---

### 16. No CSRF protection mechanism

**File**: `src/api/client.ts`  
**Severity**: Low (given Bearer token auth)

Since the app uses Bearer tokens in the Authorization header (not cookies), CSRF is not a direct risk. However, if the auth mechanism ever changes to cookies, CSRF protection would be needed. Document this assumption.

---

### 17. Test mock handler has hardcoded credentials

**File**: `src/test/mocks/handlers.ts`, line 29  
**Severity**: Low

```typescript
if (body.username === 'admin' && body.password === 'Admin@123456') {
```

This is fine for tests but ensure these values never appear in production configuration or documentation that could be mistaken for real credentials.

---

## Positive Observations

1. **TypeScript strict mode** enabled with `noUnusedLocals` and `noUnusedParameters` - good discipline.
2. **Zustand** is a solid choice for this scale - minimal boilerplate, no provider nesting.
3. **MSW-based testing** provides realistic API mocking without coupling tests to implementation.
4. **Accessibility** - proper `aria-label`, `aria-current="page"`, `role="alert"`, `aria-live="polite"` usage throughout.
5. **Responsive design** - mobile/tablet/desktop breakpoints handled consistently.
6. **Input validation** - both client-side (immediate feedback) and server-side error handling.
7. **Debounced search** in AdminUsers prevents excessive API calls.
8. **Loading skeletons** provide good perceived performance.
9. **Test coverage** is comprehensive for a Phase 1 - covers auth flow, protected routes, CRUD operations, validation, and error states.

---

## Recommendations for Phase 2

1. Add an `ErrorBoundary` component.
2. Consolidate duplicate response types.
3. Add token expiration handling (proactive logout/refresh).
4. Consider extracting form logic into custom hooks or react-hook-form.
5. Add E2E tests with Playwright (script exists but no test files found).
6. Add CSP headers in the deployment configuration.
7. Consider a `rowKey` prop for DataTable to avoid index-based keys.
