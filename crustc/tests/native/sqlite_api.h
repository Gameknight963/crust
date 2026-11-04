/* Minimal SQLite API fixture for frontend checks without a download. */
#pragma once
struct sqlite3;
struct sqlite3_stmt;
int sqlite3_open(const char*, struct sqlite3**);
int sqlite3_close(struct sqlite3*);
int sqlite3_prepare_v2(struct sqlite3*, const char*, int, struct sqlite3_stmt**, const char**);
int sqlite3_finalize(struct sqlite3_stmt*);
int sqlite3_step(struct sqlite3_stmt*);
int sqlite3_reset(struct sqlite3_stmt*);
int sqlite3_bind_int64(struct sqlite3_stmt*, int, long long);
int sqlite3_column_count(struct sqlite3_stmt*);
int sqlite3_column_type(struct sqlite3_stmt*, int);
long long sqlite3_column_int64(struct sqlite3_stmt*, int);
#define SQLITE_OK 0
#define SQLITE_ERROR 1
#define SQLITE_MISUSE 21
#define SQLITE_RANGE 25
#define SQLITE_MISMATCH 20
#define SQLITE_INTEGER 1
#define SQLITE_ROW 100
#define SQLITE_DONE 101
#define SQLITE_CONSTRAINT 19
#define SQLITE_CANTOPEN 14
