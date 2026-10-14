# ── Stage 1: build the static site ──────────────────────────────────────────
FROM node:24.21.0-alpine AS build
WORKDIR /app

COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund

COPY . .
# Public URL the site is served from, for canonical links, e.g. https://exchange.example.com
ARG SITE_ORIGIN=http://localhost
ENV SITE_ORIGIN=$SITE_ORIGIN
RUN npm run build \
 && node scripts/csp-hashes.mjs dist > csp-script-hashes

# ── Stage 2: serve it with Caddy ────────────────────────────────────────────
FROM caddy:2.11.7-alpine

COPY Caddyfile /etc/caddy/Caddyfile
COPY --from=build /app/csp-script-hashes /etc/caddy/csp-script-hashes
COPY --from=build /app/dist /srv

EXPOSE 80

# The Caddyfile reads the inline-script hashes for its CSP from the environment.
CMD ["sh", "-c", "export CSP_SCRIPT_HASHES=\"$(cat /etc/caddy/csp-script-hashes)\" && exec caddy run --config /etc/caddy/Caddyfile --adapter caddyfile"]
