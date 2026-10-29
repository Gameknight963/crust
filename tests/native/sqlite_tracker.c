/* Observe the real SQLite library through the example's imported functions. */
static int sqlite_live_databases;
static int sqlite_live_statements;
static int sqlite_allocation_budget = -1;
static sqlite3_mem_methods sqlite_allocator;

void sqlite_fail_after(int budget) { sqlite_allocation_budget = budget; }

static int sqlite_allow_allocation(void) {
    if (sqlite_allocation_budget == 0) return 0;
    if (sqlite_allocation_budget > 0) --sqlite_allocation_budget;
    return 1;
}

static void *sqlite_test_malloc(int bytes) {
    return sqlite_allow_allocation() ? sqlite_allocator.xMalloc(bytes) : NULL;
}

static void *sqlite_test_realloc(void *pointer, int bytes) {
    return sqlite_allow_allocation() ? sqlite_allocator.xRealloc(pointer, bytes) : NULL;
}

int tracked_sqlite_open(const char *path, sqlite3 **database) {
    int code = sqlite3_open(path, database);
    if (*database) ++sqlite_live_databases;
    return code;
}

int tracked_sqlite_close(sqlite3 *database) {
    int code = sqlite3_close(database);
    if (code != SQLITE_OK) abort();
    if (database) --sqlite_live_databases;
    return code;
}

int tracked_sqlite_prepare(sqlite3 *database, const char *sql, int bytes,
                           sqlite3_stmt **statement, const char **tail) {
    int code = sqlite3_prepare_v2(database, sql, bytes, statement, tail);
    if (*statement) ++sqlite_live_statements;
    return code;
}

int tracked_sqlite_finalize(sqlite3_stmt *statement) {
    int code = sqlite3_finalize(statement);
    if (statement) --sqlite_live_statements;
    return code;
}

int main(void) {
    if (sqlite3_config(SQLITE_CONFIG_GETMALLOC, &sqlite_allocator) != SQLITE_OK) abort();
    sqlite3_mem_methods allocator = sqlite_allocator;
    allocator.xMalloc = sqlite_test_malloc;
    allocator.xRealloc = sqlite_test_realloc;
    if (sqlite3_config(SQLITE_CONFIG_MALLOC, &allocator) != SQLITE_OK) abort();
    int result = crust_sqlite_test_main();
    if (sqlite_live_databases != 0 || sqlite_live_statements != 0) abort();
    if (sqlite3_shutdown() != SQLITE_OK || sqlite3_memory_used() != 0) abort();
    return result;
}
