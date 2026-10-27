int advance(int& count) {
    count += 1;
    return count < 3;
}

int main() {
    int count = 0;
    do {
        const int& current = &count;
        print(current);
        continue;
    } while (advance(&count));
    return 0;
}
