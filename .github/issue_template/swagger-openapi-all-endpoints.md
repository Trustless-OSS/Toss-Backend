## Apply Swagger/OpenAPI documentation to all API endpoints

## 📝 Description

Swagger UI is available at `/swagger` (OpenAPI JSON at `/api-doc/openapi.json`), but `ApiDoc` in `src/docs/openapi.rs` currently only documents the health endpoints:

- `GET /health` / `GET /api/health`
- `GET /api/health/database`
- `GET /api/health/redis`
- `GET /api/health/trustless-work`

All other real API routes (repos, contributor/wallet, bounty, escrow, GitHub webhook, queue stats, and root `/`) are registered in Axum but missing `#[utoipa::path]` annotations and are not listed in the OpenAPI `paths` / `components` registries. This makes `/swagger` incomplete for API consumers and frontend developers.

The goal is to document every public HTTP endpoint in OpenAPI so Swagger UI reflects the full API surface, including request/response schemas, status codes, and auth requirements where applicable.

## ✅ Requirements

- Add `#[utoipa::path]` (and related schema derives) to every public API handler that is missing documentation.
- Register all documented handlers and schemas in `src/docs/openapi.rs` (`ApiDoc` `paths` and `components`).
- Cover at least these route groups:
  - Root: `GET /`
  - Health: existing health + dependency endpoints (keep current docs)
  - Queue: `GET /api/queue/stats`
  - Repos: `/api/repos` (list, connect, sync-installation, details, delete, issues, rewards)
  - Contributor: `/api/wallet/connect`, `/api/contributor/me`
  - Bounty: `/api/milestones/push`, `/api/issues/{issueId}/retry`
  - Escrow: create-unsigned, submit-deploy, fund-unsigned, submit-fund, refund, close-unsigned, submit-close
  - GitHub: `POST /api/webhooks/github`
- Document HTTP method, path params, query params, request bodies, response bodies, and relevant status codes (`200`, `201`, `400`, `401`, `403`, `404`, `503`, etc.).
- Mark authenticated endpoints clearly (e.g. bearer security scheme) where middleware requires auth.
- Ensure `cargo check` / build still succeeds and `/swagger` loads without schema errors.
- Prefer reusing existing request/response types with `utoipa::ToSchema` instead of duplicating DTOs.
- **Very important:** Attach screenshot evidence **or** a short video proving Swagger UI shows the full API (all endpoint groups visible in `/swagger`). PRs without visual proof should not be considered complete.

## 🎯 Acceptance Criteria

- [ ] Every public Axum route appears as a path in `/api-doc/openapi.json`.
- [ ] `/swagger` lists all endpoint groups above with usable try-it-out docs (where auth allows).
- [ ] Request/response schemas for documented endpoints are present under OpenAPI `components.schemas`.
- [ ] Auth-protected endpoints declare a security scheme (e.g. Bearer JWT) in OpenAPI.
- [ ] Existing health OpenAPI docs remain correct and still compile.
- [ ] Project builds successfully after the OpenAPI annotations are added.
- [ ] **Very important:** PR includes screenshot(s) or a short video of `/swagger` showing the newly documented endpoints (not health-only).

## 📁 Expected files to change/structure

- `src/docs/openapi.rs` — register all paths + schemas (+ optional security scheme)
- `src/app.rs` — OpenAPI for root handler if documented there
- `src/routes/health.rs` — already documented; adjust only if needed for consistency
- `src/routes/queue.rs` — add `utoipa::path` + schemas
- `src/modules/repo/handlers*.rs` (and related request/response types) — OpenAPI annotations
- `src/modules/contributor/` handlers / DTOs — OpenAPI annotations
- `src/modules/bounty/` handlers / service entrypoints — OpenAPI annotations
- `src/modules/escrow/handler.rs` (+ request/response types) — OpenAPI annotations
- `src/modules/github/routes.rs` (webhook handler) — OpenAPI annotations
- `README.md` — optional note that full API docs live at `/swagger`
-
- ***
