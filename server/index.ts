import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath } from "bun";
import cors from "cors";
import express from "express";
import { Server } from "socket.io";
import {
	NumberDictionary,
	adjectives,
	animals,
	uniqueNamesGenerator,
} from "unique-names-generator";
import { z } from "zod";

const app = express();
const server = createServer(app);
const io = new Server(server, {
	cors: {
		origin: "*",
		methods: ["GET", "POST"],
	},
});

app.use(
	cors({
		origin: "*",
	}),
);

const __dirname = dirname(fileURLToPath(import.meta.url));

const getRoomName = () => {
	const numbers = NumberDictionary.generate({ min: 0, max: 9, length: 1 });

	return uniqueNamesGenerator({
		dictionaries: [numbers, adjectives, animals],
		separator: "-",
	});
};

app.get("/", (_req, res) => {
	res.sendFile(join(__dirname, "index.html"));
});

io.on("connection", (socket) => {
	// console.log(`<-- ${socket.id} connected`);

	// socket.on("disconnect", () => {
	// 	// console.log(`<-- ${socket.id} disconnected`);
	// });

	// socket.onAny((event, ...args) => {
	// // 	console.log(`<-- ${JSON.stringify(event)}`, ...args);
	// });

	// socket.onAnyOutgoing((event, ...args) => {
	// // 	console.log(`--> ${JSON.stringify(event)}`, ...args);
	// });

	socket.on("host", () => {
	// 	console.log("host");
		const room = getRoomName();

		socket.join(room);
		socket.emit("room", room);
	});

	socket.on("command", (data) => {
		const { command, room } = z
			.object({
				command: z.string(),
				room: z.string(),
			})
			.parse(data);

		socket.join(room);

		io.to(room).emit("command", command);
	});

	socket.on("response", (data) => {
		const { response, room } = z
			.object({
				response: z.string(),
				room: z.string(),
			})
			.parse(data);

		io.to(room).emit("response", response);
	});

	socket.on("error", (data) => {
		const { response, room } = z
			.object({
				response: z.string(),
				room: z.string(),
			})
			.parse(data);

		io.to(room).emit("error", response);
	});
});

const PORT = 3000;
server.listen(3000, () => {
	console.log(`Server is listening on port ${PORT}`);
});
