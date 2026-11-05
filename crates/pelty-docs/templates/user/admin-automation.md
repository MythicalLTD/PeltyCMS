# Automation and cron

PeltyCMS runs its own scheduler inside `pelty-server`. You do not need a system cron entry: as long as the server runs, these tasks run.

{{tasks}}

Admin → System & settings → **Jobs** shows each task's health (healthy, late, failing, never ran), last result, last and next run, and its successes and failures in the last 24 hours. From there you can:

- **Run now**: queue a task immediately (`jobs.manage`).
- Disable a task.
- Pin a daily task to a start hour in the company time zone (Admin > General > Localisation), for example to run billing at night.

The same page also lists the background jobs themselves (emails, provisioning, webhooks, backups).

See the developer reference [Scheduled tasks](developers/scheduled-tasks.md) for the full list of job kinds.
