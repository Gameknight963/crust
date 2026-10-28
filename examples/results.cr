import "../lib/result.cr";

enum class Error { DivisionByZero };

Result<int, Error> divide(int numerator, int denominator) {
    if (denominator == 0) {
        return Result<int, Error>::failure(Error::DivisionByZero);
    }
    return Result<int, Error>::success(numerator / denominator);
}

void show(const Result<int, Error>& result) {
    switch (result) {
        case ok:
            print(result.ok);
            break;
        case error:
            print((int)result.error);
            break;
    }
}

int main() {
    Result<int, Error> success = divide(42, 2);
    Result<int, Error> failure = divide(42, 0);
    show(&success);
    show(&failure);
    return 0;
}
