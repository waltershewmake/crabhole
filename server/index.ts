import { Server } from "socket.io";
import express, { json } from "express";
import { createServer } from "node:http";
import { z } from "zod";
import {
  adjectives,
  animals,
  NumberDictionary,
  uniqueNamesGenerator,
} from "unique-names-generator";
import cors from "cors";

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
  })
);

const numbers = NumberDictionary.generate({ min: 0, max: 999999, length: 6 });

const getRoomName = () => {
  return uniqueNamesGenerator({
    dictionaries: [numbers, adjectives, animals],
    separator: "-",
  });
};

app.get("/", (_req, res) => {
  res.send("Hello World!");
});

io.on("connection", (socket) => {
  console.log("A user connected");

  socket.on("disconnect", () => {
    console.log("A user disconnected");
  });

  socket.on("host", () => {
    console.log("host");
    const room = getRoomName();

    socket.join(room);
    socket.emit("room", room);
  });

  socket.on("command", (data) => {
    console.log(`command: ${data}`);
    const { command, room } = z
      .object({
        command: z.string(),
        room: z.string(),
      })
      .parse(JSON.parse(data));

    socket.join(room);

    io.to(room).emit("command", command);
  });

  socket.on("error", (data) => {
    // console.log(`response: ${data.response}`)
    // console.log(`room: ${data.room}`)
    // console.log(`response: ${data}`)
    // const { response, room } = z.object({
    //     response: z.string(),
    //     room: z.string(),
    // }).parse(JSON.parse(data))

    io.to(data.room).emit("error", data.response);
  });

  socket.on("response", (data) => {
    // console.log(`response: ${data.response}`)
    // console.log(`room: ${data.room}`)
    // console.log(`response: ${data}`)
    // const { response, room } = z.object({
    //     response: z.string(),
    //     room: z.string(),
    // }).parse(JSON.parse(data))

    io.to(data.room).emit("response", data.response);
  });
});

server.listen(3000, () => {
  console.log("Server is listening on port 3000");
});
