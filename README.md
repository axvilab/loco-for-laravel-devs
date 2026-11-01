# loco-for-laravel-devs

A complete blog built with [Loco](https://loco.rs) 1.2 (Rust), step by step. This is the companion code for the book **Loco for Laravel Developers** by Vitaly Ax.

Every step of the book is a git tag. Each one builds, passes `cargo test`, and is clean under `cargo clippy -- -D warnings`.

| Tag | What the blog gains |
|---|---|
| [`step-01-new`](../../tree/step-01-new) | `loco new`: the generated app, routes, `/_ping` |
| [`step-02-posts-model`](../../tree/step-02-posts-model) | Posts model, migration, entity, slug and validation |
| [`step-03-public-pages`](../../tree/step-03-public-pages) | Public pages: Tera layout and partials, Markdown, pagination |
| [`step-04-tags`](../../tree/step-04-tags) | Tags: many-to-many join table, eager loading |
| [`step-05-auth-authors`](../../tree/step-05-auth-authors) | Authors: JWT in an httpOnly cookie, login page, posts belong to users |
| [`step-06-admin`](../../tree/step-06-admin) | HTMX admin: CRUD, 422 validation, flash messages |
| [`step-07-uploads`](../../tree/step-07-uploads) | Image uploads: multipart and `ctx.storage` |
| [`step-08-background-work`](../../tree/step-08-background-work) | Background worker and mailer with Tera templates |
| [`step-09-scheduler-tasks`](../../tree/step-09-scheduler-tasks) | Scheduler and tasks, persistent queue |
| [`step-10-rss-sitemap`](../../tree/step-10-rss-sitemap) | `/feed.xml`, `/sitemap.xml`, `/robots.txt` |
| [`step-11-caching`](../../tree/step-11-caching) | Page cache and ETags, measured under load |
| [`step-12-tests`](../../tree/step-12-tests) | Request and model tests, snapshots |
| [`step-13-deploy`](../../tree/step-13-deploy) | Production config on Postgres, Dockerfile, docker-compose |

## Run it

Development (SQLite, no setup):

```sh
cd blog
cargo loco start --server-and-worker   # http://localhost:5150
```

Production-like stack (Postgres, migrations, web, worker, scheduler, Mailpit):

```sh
cd blog
cp .env.example .env    # set JWT_SECRET to: openssl rand -base64 48
docker compose up --build
```

Catch up to any chapter with `git checkout step-06-admin` (any tag from the table).

## Checks

`./scripts/check.sh` runs clippy and the tests in Docker (no Rust on the host), then starts the production stack and checks that it answers. Pass tags to check them too: `./scripts/check.sh main step-06-admin`.

Versions: Loco 1.2, SeaORM 2.0, Tera 2, Axum 0.8, Rust 1.97.

## License

MIT, see [LICENSE](LICENSE).

## Українською

Повний блог на Loco 1.2, зібраний крок за кроком. Це код до книги **«Loco для Laravel-розробника»** (Vitaly Ax). Кожен розділ книги відповідає тегу з таблиці вище: `git checkout step-04-tags` повертає проєкт рівно у стан кінця розділу. Кожен тег збирається і проходить тести. Перевірка в Docker: `./scripts/check.sh`.
