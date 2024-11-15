export class Client {
    private id: string;
    public isHost: boolean;

    constructor(id: string, isHost: boolean) {
        this.id = id;
        this.isHost = isHost;
    }
}