# Geo reference data (ISO 3166)

Bundled JSON used by `GET /api/v1/public/geo/*`.

- `countries.json`: ISO 3166-1 alpha-2 + English name (249)
- `subdivisions.json`: ISO 3166-2 code + name + country (5046)

Source snapshot: [staticdata.dev](https://staticdata.dev/) (`countries` + `country-subdivisions`), slimmed to code/name fields.

Refresh:

```bash
curl -fsSL https://staticdata.dev/v1/countries.json -o /tmp/c.json
curl -fsSL https://staticdata.dev/v1/country-subdivisions.json -o /tmp/s.json
# then slim to code/name fields, or edit by hand
```

No network fetch at runtime. Ship updates with the release.
