// Forward calls, recursion, loops, and explicit types.
int main(void)
{
    print(factorial(5));

    int count = 5;
    int total = 0;
    while (count > 0) {
        total = total + count;
        count = count - 1;
    }

    print(total);
    return 0;
}

int factorial(int value)
{
    if (value <= 1) {
        return 1;
    } else {
        return value * factorial(value - 1);
    }
}
