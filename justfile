# start docker database
create-pg:
    docker-compose up -d db

# start psql shell
psql:
    psql "postgres://postgres:postgres@localhost:5432/postgres";

# diff schema.sql against existing migrations to create a new migration
create-migration MIGRATION_NAME:
    atlas migrate diff {{ MIGRATION_NAME }} \
        --dir "file://migrations" \
        --to "file://schema.sql" \
        --dev-url "docker://postgres/18/dev?search_path=public"

# apply migrations to dev server
apply-migrations:
    sqlx migrate run;
