import "database.cr";

int main() {
    uint8_t path[128]=":memory:";
    Result<Database,int> opened=Database::open(&path);
    if (!opened.is_ok()) { print(opened.error_value()); return 1; }
    Database database=move(opened).take_value();
    {
        uint8_t create_sql[128]="CREATE TABLE values_table(value INTEGER UNIQUE)";
        Result<Statement,int> prepared=database.prepare(&create_sql);
        if (!prepared.is_ok()) return 2;
        Statement statement=move(prepared).take_value();
        if (!statement.step().is_ok()) return 3;
    }
    {
        uint8_t insert_sql[128]="INSERT INTO values_table VALUES (?)";
        Result<Statement,int> prepared=database.prepare(&insert_sql);
        if (!prepared.is_ok()) return 4;
        Statement insert=move(prepared).take_value();
        for (int i=0; i<3; i++) {
            if (!insert.bind_integer(1,10+i).is_ok()) return 5;
            if (!insert.step().is_ok()) return 6;
            if (!insert.reset().is_ok()) return 7;
        }
        if (!insert.bind_integer(1,10).is_ok()) return 8;
        Result<bool,int> duplicate=insert.step();
        if (duplicate.is_ok()) return 9;
        print(duplicate.error_value()==SQLITE_CONSTRAINT);
    }
    {
        uint8_t invalid_sql[128]="SELECT FROM values_table";
        Result<Statement,int> invalid=database.prepare(&invalid_sql);
        if (invalid.is_ok()) return 10;
        print(invalid.error_value()==SQLITE_ERROR);
    }
    {
        uint8_t query_sql[128]="SELECT count(*),sum(value) FROM values_table";
        Result<Statement,int> prepared=database.prepare(&query_sql);
        if (!prepared.is_ok()) return 11;
        Statement query=move(prepared).take_value();
        Result<bool,int> row=query.step();
        if (!row.is_ok() || !row.value()) return 12;
        Result<int64_t,int> count=query.integer(0);
        Result<int64_t,int> sum=query.integer(1);
        if (!count.is_ok() || !sum.is_ok()) return 13;
        print(count.value());print(sum.value());
        Result<bool,int> done=query.step();
        if (!done.is_ok() || done.value()) return 14;
    }
    return 0;
}
