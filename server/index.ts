import { Server } from "socket.io";
import express, { json } from 'express';
import { createServer } from 'node:http';
import { z } from "zod";
import { adjectives, animals, colors, NumberDictionary, uniqueNamesGenerator } from "unique-names-generator";

const app = express();
const server = createServer(app);
const io = new Server(server);

const numbers = NumberDictionary.generate({ min: 1, max: 9})

const getRoomName = () => {
    // generate a wormhole room name
    return uniqueNamesGenerator({
        dictionaries: [
            adjectives,
            colors,
            animals,
            numbers,
        ]
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

        io.to(room).emit(command)
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