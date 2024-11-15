import { Server } from "socket.io";
import express, { json } from 'express';
import { createServer } from 'node:http';
import { z } from "zod";
import { adjectives, animals, NumberDictionary, uniqueNamesGenerator } from "unique-names-generator";
import cors from 'cors';

const app = express();
const server = createServer(app);
const io = new Server(server, {
    cors: {
        origin: '*',
        methods: ['GET', 'POST'],
    }
});

app.use(cors({
    origin: '*'
}))

const numbers = NumberDictionary.generate({ min: 0, max: 999999, length: 6})

const getRoomName = () => {
    return uniqueNamesGenerator({
        dictionaries: [
            numbers,
            adjectives,
            animals,
        ],
        separator: '-',
    })
}

app.get('/', (_req, res) => {
    res.send('Hello World!');
})

io.on('connection', (socket) => {
    console.log('A user connected');

    socket.on('disconnect', () => {
        console.log('A user disconnected');
    })

    socket.on('host', () => {
        const room = getRoomName()

        socket.join(room)
        socket.emit('room', room)
    })

    socket.on('command', (data) => {
        const { command, room } = z.object({
            command: z.string(),
            room: z.string(),
        }).parse(data)

        io.to(room).emit('command', command)
    })

    socket.on('response', (data) => {
        const { response, room } = z.object({
            response: z.string(),
            room: z.string(),
        }).parse(data)

        io.to(room).emit('response', response)
    })
})

server.listen(3000, () => {
    console.log('Server is listening on port 3000');
})