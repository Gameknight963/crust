enum Status : uint8_t {
    Pending,
    Ready = 4,
    Failed,
};

struct Job {
    Status status;

    void finish() {
        this->status = Status::Ready;
    }
};

int main() {
    Job job = { .status = Pending };
    job.finish();
    print(job.status == Ready);
    print((int)job.status);
    return 0;
}
