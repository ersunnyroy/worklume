# @worklume/api-client

A typed client for the Worklume API, shared by the web and mobile apps. Types in
`src/schema.gen.ts` are generated from `@worklume/api-contract` with `openapi-typescript`; calls go
through `openapi-fetch` (about 6 KB).

```ts
import { createApiClient, unwrap } from '@worklume/api-client';

const api = createApiClient({ baseUrl: config.apiBaseUrl, getAccessToken: () => session.token });
const health = await unwrap(api.GET('/health')); // typed from the contract
```

- Paths, parameters, bodies and responses are checked at compile time; an unknown path does
  not compile.
- `unwrap` returns the data or throws `ApiError` with `status`, the API's `code`, `fieldErrors`
  and `requestId`. The web app's error screen shows `requestId` to the user as a support
  reference.
- The access token is read on every request and sent as a bearer token; nothing is stored.

`pnpm --filter @worklume/api-client build` regenerates the types. The package is consumed as
TypeScript source, so there is no compile step, and CI fails if the committed types are stale.
